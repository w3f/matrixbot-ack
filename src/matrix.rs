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

        self.room_send(room_id, content, None).await.unwrap();

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
                is_last = true;
                (rooms.last().unwrap(), rooms.len() - 1)
            };

            let intro = if new_idx == 0 {
                "⚠️ Alert occurred!"
            } else {
                if !is_last {
                    // Notify current room that missed to acknowledge the alert.
                    debug!("Notifying current room about escalation");
                    client
                        .send_msg(
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
                        .await
                        .unwrap();
                }

                "🚨 ESCALATION OCCURRED!"
            };

            if is_last {
                warn!("Notifying final room about escalation");
            } else {
                debug!("Notifying *next* room about escalation");
            }

            client.send_msg(room_id, intro).await.unwrap();

            // Send alerts to room.
            for alert in msg.alerts {
                client.send_msg(room_id, &alert.to_string()).await.unwrap();
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
            // Check whitelisted room.
            if !self.rooms.contains(room.room_id()) {
                return;
            }

            // Ignore own messages.
            if &event.sender == room.own_user_id() {
                return;
            }

            let msg_body = if let SyncMessageEvent {
                content:
                    MessageEventContent {
                        msgtype: MessageType::Text(TextMessageEventContent { body: msg_body, .. }),
                        ..
                    },
                ..
            } = event
            {
                msg_body.to_string()
            } else {
                debug!("Received unacceptable message type from {}", event.sender);
                return;
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
                            if let Ok(id) = AlertId::parse_str(parts[1]) {
                                Command::Ack(id)
                            } else {
                                bad_msg(&room).await
                            }
                        } else {
                            bad_msg(&room).await
                        }
                    } else {
                        return;
                    }
                }
            };

            // Determine the escalation index based on ordering of rooms.
            let escalation_idx =
                if let Some(room_id) = self.rooms.iter().position(|id| id == room.room_id()) {
                    room_id
                } else {
                    // Silent return.
                    return;
                };

            // Prepare action type.
            let action = UserAction {
                escalation_idx: escalation_idx,
                command: cmd,
            };

            // Send action to processor.
            let confirmation = Processor::from_registry().send(action).await.unwrap();

            let content = AnyMessageEventContent::RoomMessage(MessageEventContent::text_plain(
                confirmation.to_string(),
            ));

            // Notify the room.
            room.send(content, None).await.unwrap();
        }
    }
}

async fn bad_msg(room: &Joined) -> Command {
    let content = AnyMessageEventContent::RoomMessage(MessageEventContent::text_plain(
        "I don't understand 🤔",
    ));

    room.send(content, None).await.unwrap();

    Command::Help
}
