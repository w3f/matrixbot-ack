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
use tokio::sync::mpsc::{self, unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::Mutex;
use url::Url;

use super::{Adapter, AdapterAlertId, AdapterName, LevelManager};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixConfig {
    homeserver: String,
    username: String,
    password: String,
    db_path: String,
    device_name: String,
    device_id: String,
    pub rooms: Vec<String>,
}

pub struct MatrixClient {
    rooms: LevelManager<RoomId>,
    client: Client,
    // An "ugly" workaround mutation rules.
    listener: Arc<Mutex<UnboundedReceiver<UserAction>>>,
}

impl MatrixClient {
    pub async fn new(config: MatrixConfig) -> Result<Self> {
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
        let rooms: Vec<RoomId> = config
            .rooms
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
    async fn notify(&self, notification: Notification) -> Result<Option<AdapterAlertId>> {
        match notification {
            Notification::Alert { context } => {
                let (pre, next) = self.rooms.level_with_prev(context.level_idx(self.name()));
            }
            Notification::Acknowledged { id, acked_by } => {
                // TODO: Notify all rooms.
            }
        }

        unimplemented!()
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
                        let action = UserAction {
                            user: User::Matrix(event.sender.to_string()),
                            // Panicing would imply bug.
                            channel_id: self.rooms.position(room.room_id()).unwrap(),
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
