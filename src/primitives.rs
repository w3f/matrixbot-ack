use crate::{unix_time, Result, adapter::AdapterName};

// TODO: Remove
use std::fmt::Display;

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct AlertId(u64);

impl AlertId {
    pub fn from_str(str: &str) -> Result<Self> {
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
    pub acked_by: Option<User>,
    pub acked_at_tmsp: Option<u64>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct AdapterContext {
    pub name: AdapterName,
    pub level_idx: usize,
    pub last_notified_tmsp: Option<u64>,
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
    pub fn to_string_matrix(&self) -> String {
        format!(
            "\
            - ID: {}\n  \
              Name: {}\n  \
              Severity: {}\n  \
              Message: {}\n  \
              Description: {}\n\
        ",
            self.id,
            self.alert.labels.alert_name,
            self.alert.labels.severity,
            self.alert.annotations.message.as_deref().unwrap_or("N/A"),
            self.alert
                .annotations
                .description
                .as_deref()
                .unwrap_or("N/A")
        )
    }
    pub fn to_string_pagerduty(&self) -> String {
        format!(
            "\
              Name: {}, \
              Severity: {},  \
              Message: {},  \
              Description: {} - \
              ID#{}\
        ",
            self.alert.labels.alert_name,
            self.alert.labels.severity,
            self.alert.annotations.message.as_deref().unwrap_or("N/A"),
            self.alert
                .annotations
                .description
                .as_deref()
                .unwrap_or("N/A"),
            self.id,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Notification {
    Alert { context: AlertContext },
    Acknowledged { id: AlertId, acked_by: User },
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Acknowledgement<T> {
    pub user: User,
    pub channel_id: T,
    pub alert_id: AlertId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "adapter", content = "user")]
pub enum User {
    Matrix(String),
    PagerDuty(String),
    #[cfg(test)]
    Mocker(String),
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

impl std::fmt::Display for UserConfirmation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        unimplemented!()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserAction {
    pub user: User,
    // TODO: Rename, use custom type.
    pub channel_id: usize,
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

#[cfg(test)]
mod tests {
    use super::*;

    impl Alert {
        pub fn test() -> Self {
            Alert {
                annotations: Annotations {
                    message: Some("Test Alert".to_string()),
                    description: Some("Test Description".to_string()),
                },
                labels: Labels {
                    severity: "Test Severity".to_string(),
                    alert_name: "Test Alert Name".to_string(),
                },
            }
        }
    }
}
