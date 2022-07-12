use super::{Adapter, AdapterAlertId, AdapterName};
use crate::primitives::{Notification, UserAction, UserConfirmation};
use crate::Result;
use google_gmail1::{hyper, hyper_rustls, oauth2, Gmail};

pub struct EmailConfig {}

pub struct EmailLevel {}

pub struct EmailClient {}

impl EmailClient {
    pub async fn new() -> Result<Self> {
        // TODO
        let secret: oauth2::ApplicationSecret = Default::default();
        let auth = oauth2::InstalledFlowAuthenticator::builder(
            secret,
            oauth2::InstalledFlowReturnMethod::HTTPRedirect,
        )
        .build()
        .await?;

        let mut hub = Gmail::new(
            hyper::Client::builder().build(
                hyper_rustls::HttpsConnectorBuilder::new().with_native_roots()
                    .https_or_http()
                    .enable_http1()
                    .enable_http2()
                    .build(),
            ),
            auth,
        );

        unimplemented!()
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