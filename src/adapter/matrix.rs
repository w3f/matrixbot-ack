use crate::primitives::{Command, Notification, User, UserAction, UserConfirmation};
use crate::Result;
use matrix_sdk::events::room::message::MessageEventContent;
use matrix_sdk::events::SyncMessageEvent;
use matrix_sdk::room::Room;
use matrix_sdk::{Client, ClientConfig, EventHandler, SyncSettings};
use ruma::events::room::message::MessageType;
use ruma::RoomId;
use std::convert::TryFrom;
use std::sync::Arc;
use tokio::sync::mpsc::{self, unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::Mutex;
use url::Url;

use super::{Adapter, AdapterName};

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
    rooms: Vec<RoomId>,
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
    async fn notify(&self, _: Notification) -> Result<()> {
        unimplemented!()
    }
    async fn respond(&self, _: UserConfirmation) -> Result<()> {
        unimplemented!()
    }
    async fn endpoint_request(&self) -> Option<UserAction> {
        let mut l = self.listener.lock().await;
        l.recv().await
    }
}

pub struct Listener {
    rooms: Vec<RoomId>,
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
                            // TODO:
                            //channel_id: room.room_id().clone(),
                            channel_id: 0,
                            command: cmd,
                        };

                        self.queue.send(action).unwrap();
                    }
                }
                Err(_err) => {
                    // TODO: Resp error
                }
            }
        }
    }
}
