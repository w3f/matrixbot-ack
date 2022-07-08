pub mod matrix;
pub mod pagerduty;

use actix::Actor;
pub use matrix::MatrixClient;
pub use pagerduty::PagerDutyClient;

pub trait Adapter {
    type Channel;
}
