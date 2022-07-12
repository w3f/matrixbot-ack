use super::{Adapter, AdapterName, AdapterAlertId};
use crate::Result;
use crate::primitives::{Notification, UserConfirmation, UserAction};

pub struct EmailClient {

}

impl EmailClient {

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
