use super::{Adapter, AdapterAlertId, LevelManager};
use crate::primitives::{
    Acknowledgement, AlertContext, AlertId, Notification, UserAction, UserConfirmation,
};
use crate::Result;
use reqwest::header::AUTHORIZATION;
use reqwest::StatusCode;
use tokio::time::{sleep, Duration};

use super::AdapterName;

const SEND_ALERT_ENDPOINT: &str = "https://events.pagerduty.com/v2/enqueue";
const RETRY_TIMEOUT: u64 = 10; // seconds

pub struct PagerDutyClient {
    levels: LevelManager<PagerDutyLevel>,
    config: PagerDutyConfig,
    client: reqwest::Client,
}

#[async_trait]
impl Adapter for PagerDutyClient {
    fn name(&self) -> AdapterName {
        AdapterName::PagerDuty
    }
    async fn notify(&self, notification: Notification) -> Result<Option<AdapterAlertId>> {
        self.handle(notification).await
    }
    async fn respond(&self, _: UserConfirmation, _level_idx: usize) -> Result<()> {
        // Ignored, no direct responses on PagerDuty.
        Ok(())
    }
    async fn endpoint_request(&self) -> Option<UserAction> {
        unimplemented!()
    }
}

impl PagerDutyClient {
    pub fn new(mut config: PagerDutyConfig, levels: Vec<PagerDutyLevel>) -> Self {
        config.api_key = format!("Token token={}", config.api_key);

        PagerDutyClient {
            levels: LevelManager::from(levels),
            config,
            client: reqwest::Client::new(),
        }
    }
    #[rustfmt::skip]
    async fn handle(&self, notification: Notification) -> Result<Option<AdapterAlertId>> {
        // Create an alert type native to the PagerDuty API.
        match notification {
            Notification::Alert { context: alert } => {
                let level = self.levels.single_level(alert.level_idx);
                let level = if let Some(level) = level {
                    level
                } else {
                    warn!("TODO");
                    return Ok(None);
                };

                let alert = new_alert_event(
                    level.integration_key.to_string(),
                    self.config.payload_source.to_string(),
                    level.payload_severity,
                    &alert,
                );

                let resp = post_alert(&self.client, self.config.api_key.as_str(), &alert).await?;

                match resp.status() {
                    StatusCode::ACCEPTED => {
                        debug!("Received ACCEPTED from PagerDuty API");
                        // TODO: Return PagerDuty specific ID
                    }
                    StatusCode::BAD_REQUEST => {
                        return Err(anyhow!("BAD_REQUEST from server"))
                    },
                    err => {
                        return Err(anyhow!(
                            "unrecognized status code {:?} from server: {:?}",
                            err,
                            resp
                        ))
                    }
                }
            }
            Notification::Acknowledged { id, acked_by } => {
                // TODO: acknowledge via API.
            }
        }

        // Ignore any other type of Notification

        Ok(None)
    }
    async fn run_ack_checker(&self) {
        unimplemented!()
    }
}

/// Alert Event for the PagerDuty API.
#[derive(Debug, Clone, Serialize)]
pub struct AlertEvent {
    routing_key: String,
    event_action: EventAction,
    dedup_key: String,
    payload: Payload,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub struct Payload {
    summary: String,
    source: String,
    severity: PayloadSeverity,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
/// We only use Trigger.
pub enum EventAction {
    Trigger,
    Acknowledge,
    Resolve,
}

#[derive(Debug, Clone, Eq, PartialEq, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PayloadSeverity {
    Critical,
    Error,
    Warning,
    Info,
}

struct LogEntries {
    log_entries: Vec<LogEntry>,
}

struct LogEntry {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PagerDutyConfig {
    api_key: String,
    payload_source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct PagerDutyLevel {
    integration_key: String,
    payload_severity: PayloadSeverity,
}

fn new_alert_event(
    key: String,
    source: String,
    severity: PayloadSeverity,
    alert: &AlertContext,
) -> AlertEvent {
    AlertEvent {
        routing_key: key,
        event_action: EventAction::Trigger,
        dedup_key: format!("ID#{}", alert.id),
        payload: Payload {
            summary: alert.to_string_pagerduty(),
            source,
            severity,
        },
    }
}

async fn post_alert(
    client: &reqwest::Client,
    api_key: &str,
    alert: &AlertEvent,
) -> Result<reqwest::Response> {
    client
        .post(SEND_ALERT_ENDPOINT)
        .header(AUTHORIZATION, api_key)
        .json(alert)
        .send()
        .await
        .map_err(|err| err.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{primitives::{Alert, AlertContext}, unix_time};
    use std::env;

    #[ignore]
    #[tokio::test]
    async fn submit_alert_event() {
        // Keep those entries a SECRET!
        let integration_key = env::var("PD_SERVICE_KEY").unwrap();
        let api_key = env::var("PD_API_KEY").unwrap();

        let config = PagerDutyConfig {
            api_key,
            payload_source: "matrixbot-ack-test".to_string(),
        };

        let level = PagerDutyLevel {
            integration_key,
            payload_severity: PayloadSeverity::Warning,
        };

        let client = PagerDutyClient::new(config, vec![level]);

        let notification = Notification::Alert {
            context: AlertContext::new(unix_time().into(), Alert::test()),
        };

        let resp = client.handle(notification).await;

        // TODO
    }
}
