use crate::processor::{Command, Escalation, Processor, UserAction};
use crate::{AlertId, Result};
use actix::prelude::*;
use actix::SystemService;
use matrix_sdk::events::room::message::MessageEventContent;
use matrix_sdk::events::SyncMessageEvent;
use matrix_sdk::room::{Joined, Room};
use matrix_sdk::{Client, ClientConfig, EventHandler, SyncSettings};
use ruma::events::room::message::{MessageType, TextMessageEventContent};
use ruma::events::AnyMessageEventContent;
use ruma::RoomId;
use std::convert::TryFrom;
use std::sync::Arc;
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixConfig {
    homeserver: String,
    username: String,
    password: String,
    db_path: String,
    device_name: String,
    device_id: String,
}

#[derive(Clone)]
pub struct MatrixClient {
    rooms: Arc<Vec<RoomId>>,
    client: Arc<Client>,
}

impl MatrixClient {
    pub async fn new(config: &MatrixConfig, rooms: Vec<String>) -> Result<Self> {
        info!("Setting up Matrix client");
        // Setup client
        let client_config = ClientConfig::new().store_path(&config.db_path);

        let url = Url::parse(&config.homeserver)?;
        let client = Client::new_with_config(url, client_config)?;

        info!("Login with credentials");
        client
            .login(
                &config.username,
                &config.password,
                Some(&config.device_id),
                Some(&config.device_name),
            )
            .await?;

        // Sync up, avoid responding to old messages.
        info!("Syncing client");
        client.sync_once(SyncSettings::default()).await?;

        debug!("Attempting to parse room ids");
        let rooms: Vec<RoomId> = rooms
            .into_iter()
            .map(|room| RoomId::try_from(room).map_err(|err| err.into()))
            .collect::<Result<Vec<RoomId>>>()?;

        // Add event handler
        client
            .set_event_handler(Box::new(Listener {
                rooms: rooms.clone(),
            }))
            .await;

        // Start backend syncing service
        info!("Executing background sync");
        let settings = SyncSettings::default().token(
            client
                .sync_token()
                .await
                .ok_or(anyhow!("Failed to acquire sync token"))?,
        );

        // Sync in background.
        let t_client = client.clone();
        actix::spawn(async move {
            t_client.clone().sync(settings).await;
        });

        Ok(MatrixClient {
            rooms: Arc::new(rooms),
            client: Arc::new(client),
        })
    }
}

/// Convenience trait.
#[async_trait]
trait SendMsg {
    async fn send_msg(&self, room_id: &RoomId, msg: &str) -> Result<()>;
}

// Implement for matrix client.
#[async_trait]
impl SendMsg for Client {
    async fn send_msg(&self, room_id: &RoomId, msg: &str) -> Result<()> {
        let content = AnyMessageEventContent::RoomMessage(MessageEventContent::text_plain(msg));

        self.room_send(room_id, content, None).await?;

        Ok(())
    }
}

impl Default for MatrixClient {
    fn default() -> Self {
        panic!("Matrix client was not initialized in system registry. This is a bug.");
    }
}

impl Actor for MatrixClient {
    type Context = Context<Self>;
}

impl Handler<Escalation> for MatrixClient {
    type Result = ResponseActFuture<Self, Result<usize>>;

    fn handle(&mut self, msg: Escalation, _ctx: &mut Self::Context) -> Self::Result {
        let client = Arc::clone(&self.client);
        let rooms = Arc::clone(&self.rooms);

        let f = async move {
            // Determine which rooms to send the alerts to.
            let mut is_last = false;
            let (room_id, new_idx) = if let Some(room_id) = rooms.get(msg.escalation_idx) {
                (room_id, msg.escalation_idx)
            } else {
                // The last room has been reached, no longer increment room index.
                is_last = true;
                // (Regarding unwrap: the application checks on startup whether rooms have been configured).
                (rooms.last().unwrap(), rooms.len() - 1)
            };

            // Normal alert on the first room.
            let intro = if new_idx == 0 {
                "⚠️ Alert occurred!"
            }
            // Notify about escalation on further rooms.
            else {
                // No further rooms to inform if the final room has been reached.
                if !is_last {
                    // Notify current room that missed to acknowledge the alert.
                    debug!("Notifying current room about escalation");
                    client
                        .send_msg(
                            // (Id is never below zero, case handled above)
                            rooms.get(new_idx - 1).unwrap(),
                            &format!(
                                "🚨 ESCALATION OCCURRED! Notifying next room regarding Alerts: {}",
                                {
                                    let mut list = String::new();
                                    for alert in &msg.alerts {
                                        list.push_str(&format!("{}, ", alert.to_string()));
                                    }

                                    list.pop();
                                    list.pop();
                                    list
                                }
                            ),
                        )
                        .await?
                }

                "🚨 ESCALATION OCCURRED!"
            };

            if is_last {
                warn!("Notifying final room about escalation");
            } else {
                debug!("Notifying *next* room about escalation");
            }

            client.send_msg(room_id, intro).await?;

            // Send alerts to room.
            for alert in msg.alerts {
                client.send_msg(room_id, &alert.to_string()).await?;
            }

            Ok(new_idx)
        };

        Box::pin(f.into_actor(self))
    }
}

impl SystemService for MatrixClient {}
impl Supervised for MatrixClient {}

pub struct Listener {
    rooms: Vec<RoomId>,
}

#[async_trait]
impl EventHandler for Listener {
    async fn on_room_message(&self, room: Room, event: &SyncMessageEvent<MessageEventContent>) {
        if let Room::Joined(room) = room {
            let res = |room: Joined, event: SyncMessageEvent<MessageEventContent>| async move {
                // Ignore own messages.
                if &event.sender == room.own_user_id() {
                    return Ok(());
                }

                let msg_body = if let SyncMessageEvent {
                    content:
                        MessageEventContent {
                            msgtype:
                                MessageType::Text(TextMessageEventContent { body: msg_body, .. }),
                            ..
                        },
                    ..
                } = event
                {
                    msg_body.to_string()
                } else {
                    return Err(anyhow!(
                        "Received unacceptable message type from {}",
                        event.sender
                    ));
                };

                debug!("Received message from {}: {}", event.sender, msg_body);

                // For convenience.
                let msg_body = msg_body.replace("  ", " ");

                let cmd = match msg_body.trim() {
                    "pending" => Command::Pending,
                    "help" => Command::Help,
                    txt @ _ => {
                        if txt.starts_with("ack") || txt.starts_with("acknowledge") {
                            let parts: Vec<&str> = txt.split(" ").collect();
                            if parts.len() == 2 {
                                if let Ok(id) = AlertId::from_str(parts[1]) {
                                    Command::Ack(id)
                                } else {
                                    bad_msg(&room).await?
                                }
                            } else {
                                bad_msg(&room).await?
                            }
                        } else {
                            // Ignore casual chatter in rooms.
                            return Ok(());
                        }
                    }
                };

                // Determine the escalation index based on ordering of rooms.
                let escalation_idx =
                    if let Some(room_id) = self.rooms.iter().position(|id| id == room.room_id()) {
                        room_id
                    } else {
                        // Silent return.
                        return Ok(());
                    };

                // Prepare action type.
                let action = UserAction {
                    escalation_idx: escalation_idx,
                    command: cmd,
                };

                // Send action to processor.
                let confirmation = Processor::from_registry().send(action).await?;

                let content = AnyMessageEventContent::RoomMessage(MessageEventContent::text_plain(
                    confirmation.to_string(),
                ));

                // Notify the room.
                debug!("Notifying room");
                room.send(content, None).await?;

                Result::<()>::Ok(())
            };

            // Only process whitelisted rooms.
            if !self.rooms.contains(room.room_id()) {
                return;
            }

            match res(room, event.clone()).await {
                Ok(_) => {}
                Err(err) => error!("{:?}", err),
            }
        }
    }
}

async fn bad_msg(room: &Joined) -> Result<Command> {
    let content = AnyMessageEventContent::RoomMessage(MessageEventContent::text_plain(
        "I don't understand 🤔",
    ));

    room.send(content, None).await?;

    Ok(Command::Help)
}
