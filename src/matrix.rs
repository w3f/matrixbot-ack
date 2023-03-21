use crate::processor::{
    AlertContextTrimmed, Command, Escalation, NotifyAlert, Processor, UserAction,
};
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
    pub async fn new(
        config: &MatrixConfig,
        rooms: Vec<String>,
        handle_user_command: bool,
    ) -> Result<Self> {
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
        if handle_user_command {
            client
                .set_event_handler(Box::new(Listener {
                    rooms: rooms.clone(),
                }))
                .await;
        }

        // Start backend syncing service
        info!("Executing background sync");
        let settings = SyncSettings::default().token(
            client
                .sync_token()
                .await
                .ok_or_else(|| anyhow!("Failed to acquire sync token"))?,
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

/// Handler for alerts on first entry, when the webhook gets called by the
/// Watcher. Can be either an escalating or non-escalating alert.
impl Handler<NotifyAlert> for MatrixClient {
    type Result = ResponseActFuture<Self, Result<()>>;

    fn handle(&mut self, notify: NotifyAlert, _ctx: &mut Self::Context) -> Self::Result {
        let client = Arc::clone(&self.client);
        let rooms = Arc::clone(&self.rooms);

        let f = async move {
            if notify.alerts.is_empty() {
                return Ok(());
            }

            let current_room_id = rooms.get(0).unwrap_or_else(|| rooms.last().unwrap());

            let mut msg = String::from("⚠️ Alert occurred!\n\n");

            // Send alerts to room.
            for alert in notify.alerts {
                let content = if alert.should_escalate() {
                    alert.to_string()
                } else {
                    // If the alert should not escalate, send trimmed version (no Id).
                    AlertContextTrimmed::from(alert).to_string()
                };

                msg.push_str(&format!("{}\n\n", content));
            }

            msg.pop();
            msg.pop();

            client.send_msg(current_room_id, &msg).await
        };

        Box::pin(f.into_actor(self))
    }
}

/// Handler for escalations triggered by the Processor event loop. *Must* only
/// process escalating alerts or an error is returned.
impl Handler<Escalation> for MatrixClient {
    type Result = ResponseActFuture<Self, Result<bool>>;

    fn handle(&mut self, notify: Escalation, _ctx: &mut Self::Context) -> Self::Result {
        let client = Arc::clone(&self.client);
        let rooms = Arc::clone(&self.rooms);

        let f = async move {
            if notify.alerts.is_empty() {
                return Ok(false);
            }

            // Determine which rooms to send the alerts to.
            let current_room_id = rooms
                .get(notify.escalation_idx.saturating_sub(1))
                .unwrap_or_else(|| rooms.last().unwrap());

            let next_room_id = rooms
                .get(notify.escalation_idx)
                .unwrap_or_else(|| rooms.last().unwrap());

            let is_last = current_room_id == next_room_id;

            // No further rooms to inform if the final room has been reached.
            if !is_last {
                // Notify current room that missed to acknowledge the alert.
                debug!("Notifying current room about escalation");
                client
                    .send_msg(
                        current_room_id,
                        &format!(
                            "🚨 ESCALATION OCCURRED! Notifying next room regarding Alerts: {}",
                            {
                                let mut list = String::new();
                                for alert in &notify.alerts {
                                    list.push_str(&format!("ID: {}, ", alert.id.to_string()));
                                }

                                list.pop();
                                list.pop();
                                list
                            }
                        ),
                    )
                    .await?
            }

            let mut msg = String::from("🚨 ESCALATION OCCURRED!\n\n");

            if is_last {
                warn!("Notifying final room about escalation");
            } else {
                debug!("Notifying *next* room about escalation");
            }

            // Send alerts to room.
            for alert in notify.alerts {
                if !alert.should_escalate() {
                    return Err(anyhow!(
                        "Received an alert that shouldn't escalate as an escalation message"
                    ));
                }

                msg.push_str(&format!("{}\n\n", alert.to_string()));
            }

            msg.pop();
            msg.pop();

            client.send_msg(next_room_id, &msg).await?;

            Ok(is_last)
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
                    txt => {
                        if txt.to_lowercase().starts_with("ack") || txt.to_lowercase().starts_with("acknowledge") {
                            let parts: Vec<&str> = txt.split(' ').collect();
                            if parts.len() == 2 {
                                if let Ok(id) = AlertId::from_str(parts[1]) {
                                    Command::Ack(id, event.sender.to_string())
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
                    escalation_idx,
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
