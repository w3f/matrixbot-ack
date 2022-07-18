use super::{Adapter, AdapterName, LevelManager};
use crate::primitives::{Command, Notification, User, UserAction, UserConfirmation};
use crate::Result;
use matrix_sdk::events::room::message::MessageEventContent;
use matrix_sdk::events::{AnyMessageEventContent, SyncMessageEvent};
use matrix_sdk::room::Room;
use matrix_sdk::{Client, ClientConfig, EventHandler, SyncSettings};
use ruma::events::room::message::MessageType;
use ruma::RoomId;
use std::convert::TryFrom;
use std::sync::Arc;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::Mutex;

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

pub struct MatrixClient {
    rooms: LevelManager<RoomId>,
    client: Client,
    // An "ugly" workaround mutation rules.
    listener: Arc<Mutex<UnboundedReceiver<UserAction>>>,
}

impl MatrixClient {
    pub async fn new(config: MatrixConfig, rooms: Vec<String>) -> Result<Self> {
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

        let rooms = LevelManager::from(rooms);

        // Add event handler
        let (tx, listener) = unbounded_channel();
        client
            .set_event_handler(Box::new(Listener {
                rooms: rooms.clone(),
                queue: tx,
            }))
            .await;

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
        tokio::spawn(async move {
            t_client.clone().sync(settings).await;
        });

        Ok(MatrixClient {
            rooms,
            client,
            listener: Arc::new(Mutex::new(listener)),
        })
    }
}

#[async_trait]
impl Adapter for MatrixClient {
    fn name(&self) -> AdapterName {
        AdapterName::Matrix
    }
    async fn notify(&self, notification: Notification, level_idx: usize) -> Result<()> {
        match notification {
            Notification::Alert { context } => {
                let (prev, next) = self.rooms.level_with_prev(level_idx);

                // Notify previous room about escalation.
                if let Some(prev) = prev {
                    let prev = self
                        .client
                        .get_joined_room(prev)
                        .ok_or_else(|| anyhow!("failed to access room {:?}", prev))?;

                    let content = AnyMessageEventContent::RoomMessage(
                        MessageEventContent::text_plain(format!(
                            "Escalation occurred! Notifying next room about escalation ID {}",
                            context.id
                        )),
                    );

                    // Send message to room
                    prev.send(content, None).await?;
                }

                let prefix = if prev.is_some() {
                    "Escalation occurred:\n".to_string()
                } else {
                    "Alert occured:\n".to_string()
                };

                // Notify next room about escalation with the actual alert.
                let content = AnyMessageEventContent::RoomMessage(MessageEventContent::text_plain(
                    format!("{prefix}{}", context.to_string_with_newlines()),
                ));

                let next = self
                    .client
                    .get_joined_room(next)
                    .ok_or_else(|| anyhow!("failed to access room {:?}", next))?;
                next.send(content, None).await?;
            }
            Notification::Acknowledged {
                id: alert_id,
                acked_by,
                acked_on,
            } => {
                for room_id in self.rooms.all_up_to_excluding(level_idx, acked_on) {
                    let room = self.client.get_joined_room(room_id).ok_or_else(|| {
                        anyhow!("Failed to get room from Matrix on ID {:?}", room_id)
                    })?;

                    let content =
                        AnyMessageEventContent::RoomMessage(MessageEventContent::text_plain(
                            format!("Alert {} was acknowleged by {}", alert_id, acked_by),
                        ));

                    // Send message to room.
                    room.send(content, None).await?;
                }
            }
        }

        Ok(())
    }
    async fn respond(&self, resp: UserConfirmation, level_idx: usize) -> Result<()> {
        let room_id = self.rooms.single_level(level_idx);
        let room = self
            .client
            .get_joined_room(room_id)
            .ok_or_else(|| anyhow!("Failed to get room from Matrix for index {}", level_idx))?;

        let content =
            AnyMessageEventContent::RoomMessage(MessageEventContent::text_plain(resp.to_string()));

        room.send(content, None)
            .await
            .map(|_| ())
            .map_err(|err| err.into())
    }
    async fn endpoint_request(&self) -> Option<UserAction> {
        let mut l = self.listener.lock().await;
        l.recv().await
    }
}

pub struct Listener {
    rooms: LevelManager<RoomId>,
    queue: UnboundedSender<UserAction>,
}

#[async_trait]
impl EventHandler for Listener {
    async fn on_room_message(&self, room: Room, event: &SyncMessageEvent<MessageEventContent>) {
        if let Room::Joined(room) = room {
            // Only process whitelisted rooms.
            if !self.rooms.contains(room.room_id()) {
                return;
            }

            // Try to retrieve text message. If the message is not text based,
            // then just ignore.
            let msg = if let MessageType::Text(text) = &event.content.msgtype {
                text.body.clone()
            } else {
                return;
            };

            // Try to parse text message as a command. Ignore if no command was
            // recognized.
            match Command::from_string(msg) {
                Ok(try_cmd) => {
                    if let Some(cmd) = try_cmd {
                        let user = event.sender.to_string();

                        debug!("Detected valid command by {}: {:?}", user, cmd);

                        let action = UserAction {
                            user: User::Matrix(user),
                            // Panicing would imply bug.
                            channel_id: self.rooms.position(room.room_id()).unwrap(),
                            is_last_channel: self.rooms.is_last(room.room_id()),
                            command: cmd,
                        };

                        self.queue.send(action).unwrap();
                    }
                }
                Err(_err) => {
                    // Ignore unrecognized commands/talk.
                }
            }
        }
    }
}
