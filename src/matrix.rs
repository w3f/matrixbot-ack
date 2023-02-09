use crate::processor::{
    AlertContextTrimmed, Command, Escalation, NotifyAlert, Processor, UserAction,
};
use crate::{AlertId, Result};
use actix::prelude::*;
use actix::SystemService;
use matrix_sdk::config::SyncSettings;
use matrix_sdk::room::{Joined, Room};
use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;
use matrix_sdk::ruma::events::room::message::{
    MessageType, OriginalSyncRoomMessageEvent, TextMessageEventContent,
};
use matrix_sdk::ruma::RoomId;
use matrix_sdk::Client;
use std::convert::TryFrom;
use std::sync::Arc;

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
    rooms: Arc<Vec<String>>,
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
        let client = Client::builder()
            .homeserver_url(&config.homeserver)
            .sled_store(&config.db_path, None)
            .build()
            .await?;

        info!("Login with credentials");
        client
            .login_username(&config.username, &config.password)
            .device_id(&config.device_id)
            .initial_device_display_name(&config.device_name)
            .await?;

        let rooms = Arc::new(rooms);

        // Add event handler.
        let t_rooms = Arc::clone(&rooms);
        /*
        client.add_event_handler(|event: OriginalSyncRoomMessageEvent, room: Room| async move {
            message_handler(event, room, t_rooms)
        });
        */

        // Sync up, avoid responding to old messages.
        info!("Syncing client");
        client.sync(SyncSettings::default()).await?;

        Ok(MatrixClient {
            rooms,
            client: Arc::new(client),
        })
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

            let room = client
                .get_joined_room(&RoomId::parse(&current_room_id)?)
                .ok_or(anyhow!("failed to retrieve room"))?;
            room.send(RoomMessageEventContent::text_plain(&msg), None)
                .await?;

            Ok(())
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
                let room = client
                    .get_joined_room(&RoomId::parse(&current_room_id)?)
                    .ok_or(anyhow!("failed to retrieve joined room"))?;
                room.send(
                    RoomMessageEventContent::text_plain(&format!(
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
                    )),
                    None,
                )
                .await?;
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

            let room = client
                .get_joined_room(&RoomId::parse(&next_room_id)?)
                .ok_or(anyhow!("failed to retrieve joined room"))?;
            room.send(RoomMessageEventContent::text_plain(&msg), None)
                .await?;

            Ok(is_last)
        };

        Box::pin(f.into_actor(self))
    }
}

impl SystemService for MatrixClient {}
impl Supervised for MatrixClient {}

async fn message_handler(event: OriginalSyncRoomMessageEvent, room: Room, rooms: Arc<Vec<String>>) {
    if let Room::Joined(room) = room {
        let res = |room: Joined, event: OriginalSyncRoomMessageEvent, rooms: Arc<Vec<String>>| async move {
            // Ignore own messages.
            if &event.sender == room.own_user_id() {
                return Ok(());
            }

            let MessageType::Text(text_content) = event.content.msgtype else {
                return Ok(());
            };

            debug!("Received message from {}: {:?}", event.sender, text_content);

            // For convenience.
            let msg_body = text_content.body.replace("  ", " ");

            let cmd = match msg_body.trim() {
                "pending" => Command::Pending,
                "help" => Command::Help,
                txt => {
                    if txt.to_lowercase().starts_with("ack")
                        || txt.to_lowercase().starts_with("acknowledge")
                    {
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
                if let Some(room_id) = rooms.iter().position(|id| id == room.room_id()) {
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

            let content = RoomMessageEventContent::text_plain(confirmation.to_string());

            // Notify the room.
            debug!("Notifying room");
            room.send(content, None).await?;

            Result::<()>::Ok(())
        };

        // Only process whitelisted rooms.
        let room_id = room.room_id().to_string();
        if rooms.contains(&room_id) {
            return;
        }

        match res(room, event.clone(), Arc::clone(&rooms)).await {
            Ok(_) => {}
            Err(err) => error!("{:?}", err),
        }
    }
}

async fn bad_msg(room: &Joined) -> Result<Command> {
    let content = RoomMessageEventContent::text_plain("I don't understand 🤔");

    room.send(content, None).await?;

    Ok(Command::Help)
}
