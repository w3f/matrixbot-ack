pub mod matrix;
pub mod pagerduty;

use crate::primitives::{Notification, UserAction, UserConfirmation};
use crate::Result;

pub use matrix::MatrixClient;
pub use pagerduty::PagerDutyClient;
use std::fmt;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AdapterName {
    Matrix,
    PagerDuty,
}

impl fmt::Display for AdapterName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                AdapterName::Matrix => "Matrix",
                AdapterName::PagerDuty => "PagerDuty",
            }
        )
    }
}

pub struct AdapterAlertId(String);

#[async_trait]
pub trait Adapter: 'static + Send + Sync {
    fn name(&self) -> AdapterName;
    async fn notify(&self, _: Notification) -> Result<Option<AdapterAlertId>>;
    async fn respond(&self, _: UserConfirmation, level_idx: usize) -> Result<()>;
    async fn endpoint_request(&self) -> Option<UserAction>;
}

// TODO: Note about empty levels and unwraps.
#[derive(Debug, Clone)]
struct LevelManager<T> {
    levels: Vec<T>,
}

impl<T: Eq + PartialEq> LevelManager<T> {
    fn from(levels: Vec<T>) -> LevelManager<T> {
        LevelManager { levels }
    }
    fn contains(&self, level: &T) -> bool {
        self.levels.contains(level)
    }
    fn single_level(&self, level: usize) -> Option<&T> {
        self.levels.get(level)
    }
    // TODO: Rename
    fn level_with_prev(&self, level: usize) -> (Option<&T>, &T) {
        if self.levels.len() < level {
            (None, self.levels.last().unwrap())
        } else if level == 0 {
            (None, self.levels.first().unwrap())
        } else {
            (
                Some(self.levels.get(level - 1).unwrap()),
                self.levels.get(level).unwrap(),
            )
        }
    }
}
