pub mod email;
pub mod matrix;
pub mod pagerduty;

use crate::primitives::{Notification, UserAction, UserConfirmation};
use crate::Result;

pub use matrix::MatrixClient;
pub use pagerduty::PagerDutyClient;
use std::fmt;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterName {
    Matrix,
    PagerDuty,
    Email,
    #[cfg(test)]
    MockerFirst,
    #[cfg(test)]
    MockerSecond,
}

impl fmt::Display for AdapterName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                AdapterName::Matrix => "Matrix",
                AdapterName::PagerDuty => "PagerDuty",
                AdapterName::Email => "email",
                #[cfg(test)]
                AdapterName::MockerFirst => "MockerFirst",
                #[cfg(test)]
                AdapterName::MockerSecond => "MockerSecond",
            }
        )
    }
}

#[async_trait]
pub trait Adapter: 'static + Send + Sync {
    fn name(&self) -> AdapterName;
    async fn notify(&self, _: Notification, level_idx: usize) -> Result<()>;
    async fn respond(&self, _: UserConfirmation, level_idx: usize) -> Result<()>;
    async fn endpoint_request(&self) -> Option<UserAction>;
}

#[derive(Debug, Clone)]
pub struct LevelManager<T> {
    levels: Vec<T>,
}

impl<T: Eq + PartialEq> LevelManager<T> {
    fn from(levels: Vec<T>) -> LevelManager<T> {
        LevelManager { levels }
    }
    fn contains(&self, level: &T) -> bool {
        self.levels.contains(level)
    }
    fn single_level(&self, level: usize) -> &T {
        self.levels
            .get(level)
            .unwrap_or_else(|| self.levels.last().unwrap())
    }
    fn position(&self, level: &T) -> Option<usize> {
        self.levels.iter().position(|l| l == level)
    }
    fn is_last(&self, level: &T) -> bool {
        self.levels.last().as_ref().unwrap() == &level
    }
    // TODO: Rename
    fn level_with_prev(&self, level: usize) -> (Option<&T>, &T) {
        if level > self.levels.len() - 1 {
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
    fn all_up_to_excluding(&self, level_idx: usize, excluding: Option<usize>) -> Vec<&T> {
        let mut levels: Vec<&T> = self.levels.iter().take(level_idx).collect();

        if excluding.is_none() {
            return levels;
        }

        let excl = excluding.unwrap();
        if levels.len() - 1 > excl {
            levels.remove(excl);
        }

        levels
    }
}
