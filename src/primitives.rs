use crate::adapter::pagerduty::PayloadSeverity;
use crate::adapter::Adapter;
use crate::{unix_time, Result};
use ruma::RoomId;
use std::fmt::Display;

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct AlertId(u64);

impl AlertId {
    fn from_str(str: &str) -> Result<Self> {
        Ok(AlertId(str.parse()?))
    }
}

impl From<u64> for AlertId {
    fn from(val: u64) -> Self {
        AlertId(val)
    }
}

impl std::fmt::Display for AlertId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct AlertContext {
    pub id: AlertId,
    alert: Alert,
    inserted_tmsp: u64,
    level_idx: usize,
    last_notified_tmsp: Option<u64>,
    acked_by: Option<User>,
    acked_at_tmsp: Option<u64>,
}

impl AlertContext {
    pub fn new(id: AlertId, alert: Alert) -> Self {
        AlertContext {
            id,
            alert,
            inserted_tmsp: unix_time(),
            level_idx: 0,
            last_notified_tmsp: None,
            acked_by: None,
            acked_at_tmsp: None,
        }
    }
    pub fn into_escalation<T: Adapter>(
        self,
        levels: &[<T as Adapter>::Channel],
    ) -> (Escalation<T>, usize) {
        // Unwraps in this method will only panic if `levels` is
        // empty, which is checked for on application startup. I.e. panicing
        // indicates a bug.

        // TODO: Document
        let (prev_room, channel_id) = {
            if self.level_idx == 0 && self.last_notified_tmsp.is_none() {
                (None, levels.get(self.level_idx).cloned().unwrap())
            } else {
                (
                    levels.get(self.level_idx).cloned(),
                    levels
                        .get(self.level_idx + 1)
                        .or_else(|| levels.last())
                        .cloned()
                        .unwrap(),
                )
            }
        };

        (
            Escalation {
                id: self.id,
                alert: self.alert,
                prev_room,
                channel_id,
            },
            {
                if self.level_idx == levels.len() - 1 {
                    self.level_idx
                } else {
                    self.level_idx + 1
                }
            },
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Message)]
#[rtype(result = "Result<()>")]
pub struct Escalation<T: Adapter> {
    pub id: AlertId,
    pub alert: Alert,
    pub prev_room: Option<<T as Adapter>::Channel>,
    pub channel_id: <T as Adapter>::Channel,
}

// TODO: Rename
#[derive(Clone, Debug, Eq, PartialEq, Message)]
#[rtype(result = "Result<()>")]
pub struct AlertDelivery {
    pub id: AlertId,
    pub alert: Alert,
    pub prev_room: Option<ChannelId>,
    pub channel_id: ChannelId,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Alert {
    pub annotations: Annotations,
    pub labels: Labels,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Annotations {
    pub message: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Labels {
    pub severity: String,
    #[serde(rename = "alertname")]
    pub alert_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Message)]
#[rtype(result = "()")]
pub struct NotifyNewlyInserted {
    pub alerts: Vec<AlertContext>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingAlerts {
    pub alerts: Vec<AlertContext>,
}

// TODO: Rename
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncrementedPendingAlerts {
    pub id: AlertId,
    pub new_level_idx: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Message)]
#[rtype(result = "Result<UserConfirmation>")]
pub struct Acknowledgement {
    pub user: User,
    pub channel_id: ChannelId,
    pub alert_id: AlertId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "adapter", content = "user")]
pub enum User {
    Matrix(String),
    #[cfg(test)]
    Mocker(String),
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Role(String);

impl Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UserConfirmation {
    PendingAlerts(PendingAlerts),
    NoPermission,
    _AlertOutOfScope,
    AlertAcknowledged(AlertId),
    AlertNotFound,
    Help,
    InternalError,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChannelId {
    Matrix(RoomId),
    PagerDuty {
        integration_key: String,
        payload_severity: PayloadSeverity,
    },
    #[cfg(test)]
    Mocker(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Message)]
#[rtype(result = "Result<UserConfirmation>")]
pub struct UserAction {
    pub user: User,
    pub channel_id: ChannelId,
    pub command: Command,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    Ack(AlertId),
    Pending,
    Help,
}

impl Command {
    pub fn from_string(input: String) -> Result<Option<Self>> {
        // TODO: Beautify this?
        let input = input.replace("  ", " ");
        let input = input.to_lowercase();
        let input = input.trim();

        let cmd = match input {
            "pending" => Command::Pending,
            "help" => Command::Help,
            txt => {
                if txt.starts_with("ack") || txt.starts_with("acknowledge") {
                    let parts: Vec<&str> = txt.split(' ').collect();
                    if parts.len() == 2 {
                        if let Ok(id) = AlertId::from_str(parts[1]) {
                            Command::Ack(id)
                        } else {
                            return Err(anyhow!("invalid command"));
                        }
                    } else {
                        return Err(anyhow!("invalid command"));
                    }
                } else {
                    // Ignore unrecognized commands
                    return Ok(None);
                }
            }
        };

        Ok(Some(cmd))
    }
}
