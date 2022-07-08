use crate::escalation::EscalationService;
use crate::primitives::{Command, User, UserAction};
use crate::Result;
use actix::prelude::*;
use matrix_sdk::events::room::message::MessageEventContent;
use matrix_sdk::events::SyncMessageEvent;
use matrix_sdk::room::Room;
use matrix_sdk::{Client, ClientConfig, EventHandler, SyncSettings};
use ruma::events::room::message::MessageType;
use ruma::RoomId;
use std::convert::TryFrom;
use std::sync::Arc;
use url::Url;

use super::Adapter;

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

#[derive(Clone)]
pub struct MatrixClient {
    _rooms: Arc<Vec<RoomId>>,
    _client: Arc<Client>,
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
        /* TODO
        if handle_user_command {
            client
                .set_event_handler(Box::new(Listener {
                    rooms: rooms.clone(),
                }))
                .await;
        }
         */

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
            _rooms: Arc::new(rooms),
            _client: Arc::new(client),
        })
    }
}

impl Actor for MatrixClient {
    type Context = Context<Self>;
}

pub struct Listener {
    rooms: Vec<RoomId>,
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
                            channel_id: room.room_id().clone(),
                            command: cmd,
                        };

                        // TODO: Handle
                        //let _x = self.request_handler.send(action).await;
                    }
                }
                Err(_err) => {
                    // TODO: Resp error
                }
            }
        }
    }
}
