pub mod matrix;
pub mod pagerduty;

use crate::primitives::{Notification, UserAction};
use crate::Result;
use actix::Actor;
pub use matrix::MatrixClient;
pub use pagerduty::PagerDutyClient;
use tokio::sync::mpsc::UnboundedReceiver;

#[async_trait]
pub trait Adapter: 'static + Send + Sync {
    async fn notify(&self) -> Result<()>;
    async fn endpoint_request(&self) -> Option<UserAction>;
}
