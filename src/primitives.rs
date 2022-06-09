use std::{fmt::Display, str::FromStr};

use ruma::RoomId;

use crate::{unix_time, Result};

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
    pub alert: Alert,
    pub inserted_tmsp: u64,
    pub level_idx: usize,
    pub last_notified_tmsp: Option<u64>,
}

// TODO: Rename
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AlertDelivery {
    pub id: AlertId,
    pub alert: Alert,
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

// TODO: Rename
enum NotificationLevel {
    Matrix(String),
    // TODO
    PagerDuty(()),
}

impl AlertContext {
    pub fn new(alert: Alert, id: AlertId) -> Self {
        unimplemented!()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Message)]
#[rtype(result = "()")]
pub struct NotifyNewlyInserted {
    alerts: Vec<AlertDelivery>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingAlerts {
    alerts: Vec<AlertContext>,
}

impl PendingAlerts {
    pub fn into_notifications(mut self, levels: &[ChannelId]) -> (PendingAlerts, NotifyAlert) {
        let alerts = self
            .alerts
            .iter()
            .map(|alert| {
                AlertDelivery {
                    id: alert.id,
                    alert: alert.alert.clone(),
                    // TODO: Beautify this
                    channel_id: levels
                        .get(alert.level_idx)
                        .or_else(|| levels.last().clone())
                        .map(|l| l.clone())
                        .unwrap(),
                }
            })
            .collect();

        let now = unix_time();
        self.alerts.iter_mut().for_each(|alert| {
            alert.level_idx += 1;
            alert.last_notified_tmsp = Some(now);
        });

        (self, NotifyAlert { alerts })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Message)]
#[rtype(result = "Result<()>")]
pub struct NotifyAlert {
    alerts: Vec<AlertDelivery>,
}

impl NotifyAlert {
    pub fn contexts(&self) -> &[AlertDelivery] {
        self.alerts.as_ref()
    }
    pub fn contexts_owned(self) -> Vec<AlertDelivery> {
        self.alerts
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Message)]
#[rtype(result = "Result<UserConfirmation>")]
pub struct Acknowledgement {
    pub user: User,
    pub channel_id: ChannelId,
    pub alert_id: AlertId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum User {
    Matrix(String),
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
    AlertOutOfScope,
    AlertAcknowledged(AlertId),
    AlertNotFound,
    Help,
    InternalError,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChannelId {
    Matrix(RoomId),
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
                    let parts: Vec<&str> = txt.split(" ").collect();
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
