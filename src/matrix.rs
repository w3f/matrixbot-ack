use crate::processor::{Command, NotifyPending, Processor, UserAction, UserConfirmation};
use crate::{AlertId, Result};
use actix::prelude::*;
use actix::SystemService;
use matrix_sdk::events::room::member::MemberEventContent;
use matrix_sdk::events::room::message::MessageEventContent;
use matrix_sdk::events::{StrippedStateEvent, SyncMessageEvent};
use matrix_sdk::room::{Joined, Room};
use matrix_sdk::{Client, ClientConfig, EventHandler, SyncSettings};
use ruma::events::room::message::{MessageType, TextMessageEventContent};
use ruma::events::AnyMessageEventContent;
use ruma::RoomId;
use std::convert::TryFrom;
use std::sync::Arc;
use url::Url;

#[derive(Clone)]
pub struct MatrixClient {
    rooms: Arc<Vec<RoomId>>,
    client: Arc<Client>,
}

impl MatrixClient {
    pub async fn new(
        homeserver: &str,
        username: &str,
        password: &str,
        db_path: &str,
        rooms: Vec<String>,
    ) -> Result<Self> {
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
            rooms: Arc::new(vec![]),
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

impl Handler<NotifyPending> for MatrixClient {
    type Result = ResponseActFuture<Self, Result<usize>>;

    fn handle(&mut self, msg: NotifyPending, ctx: &mut Self::Context) -> Self::Result {
        let client = Arc::clone(&self.client);
        let rooms = Arc::clone(&self.rooms);

        let f = async move {
            // Determine which rooms to send the alerts to.
            let (room_id, new_idx) = if let Some(room_id) = rooms.get(msg.escalation_idx) {
                (room_id, msg.escalation_idx)
            } else {
                (rooms.last().unwrap(), rooms.len() - 1)
            };

            if new_idx == 0 {
                client
                    .send_msg(room_id, "🚨 NEW ALERTS OCCURRED!")
                    .await
                    .unwrap();
            }

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
