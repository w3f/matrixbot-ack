pub mod matrix;
pub mod pagerduty;

use actix::Actor;
pub use matrix::MatrixClient;
pub use pagerduty::PagerDutyClient;

#[async_trait]
pub trait Adapter {
    type Channel: Clone + std::fmt::Debug + Eq + PartialEq;

    fn set_event_queue();
    async fn run_request_handler();
    async fn notify_escalation();
}
