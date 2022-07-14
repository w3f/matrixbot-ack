use super::{Adapter, AdapterAlertId, AdapterName};
use crate::primitives::{Notification, UserAction, UserConfirmation, AlertId, Command, User};
use crate::Result;
use google_gmail1::api::Message;
use google_gmail1::{hyper, hyper_rustls, oauth2, Gmail};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

pub struct EmailConfig {
    address: String,
}

pub struct EmailLevel {}

pub struct EmailClient {
    client: Gmail,
    config: EmailConfig,
    queue: UnboundedReceiver<UserAction>,
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

        unimplemented!()
        //Ok(EmailClient { client, config })
    }
    pub async fn run_message_import(&self) {
        // TODO: Add filter/max/limit
        let (_resp, list) = self
            .client
            .users()
            .messages_list(&self.config.address)
            .doit()
            .await
            .unwrap();

        for message in &list.messages.unwrap() {
            let (_resp, message) = self
                .client
                .users()
                .messages_get(&self.config.address, &message.id.as_ref().unwrap())
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
                                    let action = UserAction {
                                        user: User::Email("TODO".to_string()),
                                        channel_id: 0,
                                        command: Command::Ack(alert_id),
                                    };

                                    // TODO: send
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn some() {
    let text = "ack whatever";
    let res: Vec<&str> = text.split("ack").collect();
    println!("{:?}", res);
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
        unimplemented!()
    }
}
