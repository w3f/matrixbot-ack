use super::{Adapter, AdapterAlertId, AdapterName};
use crate::primitives::{Notification, UserAction, UserConfirmation};
use crate::Result;
use google_gmail1::api::Message;
use google_gmail1::{hyper, hyper_rustls, oauth2, Gmail};

pub struct EmailConfig {
    address: String,
}

pub struct EmailLevel {}

pub struct EmailClient {
    client: Gmail,
    config: EmailConfig,
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

        Ok(EmailClient { client, config })
    }
    pub async fn run_message_import(&self) {
        // TODO: Add filter/max/limit
        let messages = self
            .client
            .users()
            .messages_list(&self.config.address)
            .doit()
            .await
            .unwrap();

    }
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
