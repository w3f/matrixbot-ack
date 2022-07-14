use super::{Adapter, AdapterAlertId, AdapterName};
use crate::primitives::{AlertId, Command, Notification, User, UserAction, UserConfirmation};
use crate::Result;
use google_gmail1::api::{Message, MessagePart, MessagePartHeader};
use google_gmail1::{hyper, hyper_rustls, oauth2, Gmail};
use std::sync::Arc;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

const MESSAGE_IMPORT_INTERVAL: u64 = 5;

pub struct EmailConfig {
    address: String,
    max_import_days: usize,
}

pub struct EmailLevel {}

pub struct EmailClient {
    client: Arc<Gmail>,
    config: EmailConfig,
    tx: Arc<UnboundedSender<UserAction>>,
    queue: Arc<Mutex<UnboundedReceiver<UserAction>>>,
}

impl EmailClient {
    pub async fn new(config: EmailConfig) -> Result<Self> {
        // TODO
        let secret: oauth2::ApplicationSecret = Default::default();
        let auth = oauth2::InstalledFlowAuthenticator::builder(
            secret,
            oauth2::InstalledFlowReturnMethod::HTTPRedirect,
        )
        .build()
        .await?;

        let client = Gmail::new(
            hyper::Client::builder().build(
                hyper_rustls::HttpsConnectorBuilder::new()
                    .with_native_roots()
                    .https_or_http()
                    .enable_http1()
                    .enable_http2()
                    .build(),
            ),
            auth,
        );

        let (tx, queue) = unbounded_channel();

        let email = EmailClient {
            client: Arc::new(client),
            config,
            tx: Arc::new(tx),
            queue: Arc::new(Mutex::new(queue)),
        };

        // Run background task for importing emails.
        email.run_message_import().await;

        Ok(email)
    }
    async fn run_message_import(&self) {
        let client = Arc::clone(&self.client);
        let address = self.config.address.to_string();
        let tx = Arc::clone(&self.tx);
        let max_days = self.config.max_import_days;

        tokio::spawn(async move {
            if let Err(err) = Self::import_messages(&address, &client, &tx, max_days).await {
                error!("failed to import emails: {:?}", err);
            }

            sleep(Duration::from_secs(MESSAGE_IMPORT_INTERVAL)).await
        });
    }
    async fn import_messages(
        address: &str,
        client: &Arc<Gmail>,
        tx: &Arc<UnboundedSender<UserAction>>,
        max_days: usize,
    ) -> Result<()> {
        // TODO: Add filter/max/limit
        let (_resp, list) = client
            .users()
            .messages_list(address)
            .q(&format!("newer_than:{}d", max_days))
            .doit()
            .await
            .unwrap();

        for message in &list.messages.unwrap() {
            let (_resp, message) = client
                .users()
                .messages_get(address, message.id.as_ref().unwrap())
                .doit()
                .await
                .unwrap();

            if let Some(payload) = message.payload {
                if let Some(body) = payload.body {
                    if let Some(data) = body.data {
                        // TODO: Restrict this some more?
                        let text = data.to_lowercase();
                        if text.contains("ack") {
                            if let Some(id_str) = text.split("ack").nth(1) {
                                if let Ok(alert_id) = AlertId::from_str(id_str) {
                                    // Retrieve sender from 'To' field.
                                    let name;
                                    match payload.headers {
                                        Some(headers) => {
                                            let to_header = headers.iter().find(|part| {
                                                part.name
                                                    .as_ref()
                                                    .map(|name| name == "To")
                                                    .unwrap_or(false)
                                            });

                                            name = to_header
                                                .ok_or(anyhow!(""))?
                                                .value
                                                .as_ref()
                                                .ok_or(anyhow!(""))?
                                                .clone();
                                        }
                                        None => {
                                            error!("TODO");
                                            continue;
                                        }
                                    }

                                    // Create user action.
                                    let action = UserAction {
                                        user: User::Email(name),
                                        channel_id: 0,
                                        command: Command::Ack(alert_id),
                                    };

                                    tx.send(action).unwrap();
                                }
                            }
                        }
                    }
                }
            }
        }

        unimplemented!()
    }
}

fn create_message(to: &str, content: &str) -> Message {
    let mut msg = Message::default();
    let mut payload = MessagePart::default();

    let header = MessagePartHeader {
        name: Some("To".to_string()),
        value: Some(to.to_string()),
    };

    payload.headers = Some(vec![header]);
    msg.payload = Some(payload);
    msg.raw = Some(base64::encode(content));

    msg
}

#[async_trait]
impl Adapter for EmailClient {
    fn name(&self) -> AdapterName {
        AdapterName::Matrix
    }
    async fn notify(
        &self,
        notification: Notification,
        level_idx: usize,
    ) -> Result<Option<AdapterAlertId>> {
        unimplemented!()
    }
    async fn respond(&self, resp: UserConfirmation, level_idx: usize) -> Result<()> {
        unimplemented!()
    }
    async fn endpoint_request(&self) -> Option<UserAction> {
        let mut l = self.queue.lock().await;
        l.recv().await
    }
}
