use crate::processor::{Command, Processor, UserAction, UserConfirmation};
use crate::{AlertId, Result};
use actix::SystemService;
use matrix_sdk::events::room::member::MemberEventContent;
use matrix_sdk::events::room::message::MessageEventContent;
use matrix_sdk::events::{StrippedStateEvent, SyncMessageEvent};
use matrix_sdk::room::{Joined, Room};
use matrix_sdk::{Client, ClientConfig, EventHandler, SyncSettings};
use ruma::events::room::message::{MessageType, TextMessageEventContent};
use ruma::events::AnyMessageEventContent;
use ruma::RoomId;
use url::Url;

#[derive(Clone)]
pub struct MatrixClient;

impl MatrixClient {
    pub async fn new(
        homeserver: &str,
        username: &str,
        password: &str,
        db_path: &str,
    ) -> Result<()> {
        info!("Setting up Matrix client");
        // Setup client
        let client_config = ClientConfig::new().store_path(db_path);

        let homeserver = Url::parse(homeserver)?;
        let client = Client::new_with_config(homeserver, client_config)?;

        info!("Login with credentials");
        client
            .login(username, password, None, Some("w3f-registrar-bot"))
            .await?;

        // Sync up, avoid responding to old messages.
        info!("Syncing client");
        client.sync_once(SyncSettings::default()).await?;

        // Add event handler
        client
            .set_event_handler(Box::new(Listener { rooms: vec![] }))
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
        actix::spawn(async move {
            client.clone().sync(settings).await;
        });

        Ok(())
    }
}

pub struct Listener {
    rooms: Vec<RoomId>,
}

#[async_trait]
impl EventHandler for Listener {
    async fn on_room_message(&self, room: Room, event: &SyncMessageEvent<MessageEventContent>) {
        if let Room::Joined(room) = room {
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

            debug!("Received message from {}", event.sender);

            let cmd = match msg_body.trim() {
                "pending" => Command::Pending,
                "help" => Command::Help,
                txt @ _ => {
                    if txt.starts_with("ack") || txt.starts_with("acknowledge") {
                        let parts: Vec<&str> = txt.split(" ").collect();
                        if parts.len() != 2 {
                            bad_msg(&room).await;
                        }

                        if let Ok(id) = AlertId::from_bytes(parts[1].as_bytes()) {
                            Command::Ack(id)
                        } else {
                            bad_msg(&room).await
                        }
                    } else {
                        bad_msg(&room).await
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
