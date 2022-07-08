pub mod matrix;
pub mod pagerduty;

use crate::primitives::{Notification, UserAction, UserConfirmation};
use crate::Result;

pub use matrix::MatrixClient;
pub use pagerduty::PagerDutyClient;
use std::fmt;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AdapterName {
    Matrix,
}

impl fmt::Display for AdapterName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                AdapterName::Matrix => "Matrix",
            }
        )
    }
}

#[async_trait]
pub trait Adapter: 'static + Send + Sync {
    fn name(&self) -> AdapterName;
    async fn notify(&self, _: Notification) -> Result<()>;
    async fn respond(&self, _: UserConfirmation) -> Result<()>;
    async fn endpoint_request(&self) -> Option<UserAction>;
}
