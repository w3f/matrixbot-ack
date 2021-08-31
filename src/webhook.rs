use crate::{AlertId, Result};

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebhookInsert {
    alerts: Vec<Alert>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Alert {
    annotations: Annotations,
    labels: Labels,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Annotations {
    message: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Labels {
    severity: String,
    #[serde(rename = "alertname")]
    alert_name: String,
}
