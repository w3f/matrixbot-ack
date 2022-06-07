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
    pub timestamp: u64,
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
        AlertContext {
            id,
            alert,
            timestamp: unix_time(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Message)]
#[rtype(result = "()")]
pub struct NotifyNewlyInserted {
    alerts: Vec<AlertContext>,
}

#[derive(Clone, Debug, Eq, PartialEq, Message)]
#[rtype(result = "Result<()>")]
pub struct NotifyAlert {
    alerts: Vec<AlertContext>,
}

impl From<NotifyNewlyInserted> for NotifyAlert {
    fn from(val: NotifyNewlyInserted) -> Self {
        NotifyAlert { alerts: val.alerts }
    }
}

impl NotifyAlert {
    pub fn contexts(&self) -> &[AlertContext] {
        self.alerts.as_ref()
    }
    pub fn contexts_owned(self) -> Vec<AlertContext> {
        self.alerts
    }
    pub fn update_timestamp_now(&mut self) {
        let now = unix_time();

        self.alerts
            .iter_mut()
            .for_each(|alert| alert.timestamp = now);
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Message)]
#[rtype(result = "Result<UserConfirmation>")]
pub struct Acknowledgement<T> {
    pub user: User,
    pub channel_id: T,
    pub alert_id: AlertId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct User;
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Role;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UserConfirmation {
    PendingAlerts(Vec<AlertContext>),
    NoPermission,
    AlertOutOfScope,
    AlertAcknowledged(AlertId),
    AlertNotFound,
    Help,
    InternalError,
}
