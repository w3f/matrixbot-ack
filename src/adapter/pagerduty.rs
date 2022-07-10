use super::{Adapter, AdapterAlertId, LevelManager};
use crate::primitives::{
    Acknowledgement, AlertContext, AlertId, Notification, User, UserAction, UserConfirmation,
};
use crate::Result;
use reqwest::header::AUTHORIZATION;
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::time::{sleep, Duration};

use super::AdapterName;

const SEND_ALERT_ENDPOINT: &str = "https://events.pagerduty.com/v2/enqueue";
// TODO: Add note about limit
const GET_LOG_ENTRIES_ENDPOINT: &str = "https://api.pagerduty.com/log_entries?limit=20";
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

                // Send authenticated POST request. We don't care about the
                // return value as long as it succeeds.
                let _resp = auth_post::<_, serde_json::Value>(SEND_ALERT_ENDPOINT, &self.client, &self.config.api_key, &alert).await?;
            }
            Notification::Acknowledged { id, acked_by } => {
                // TODO: acknowledge via API.
            }
        }

        // Ignore any other type of Notification

        Ok(None)
    }
    async fn fetch_log_entries(&self) {
        let resp =
            auth_get::<LogEntries>(GET_LOG_ENTRIES_ENDPOINT, &self.client, &self.config.api_key)
                .await
                .unwrap();
        println!("{:?}", resp);
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LogEntries {
    log_entries: Vec<LogEntry>,
}

impl LogEntries {
    fn get_acknowledged(&self) -> Vec<(AlertId, User)> {
        let entries: Vec<&LogEntry> = self
            .log_entries
            .iter()
            // Filter for acknowledged alerts
            .filter(|entry| {
                entry
                    .ty
                    .as_ref()
                    .map(|ty| ty.contains("acknowledge_log_entry"))
                    .unwrap_or(false)
            })
            .collect();

        let mut acks = vec![];

        for entry in entries {
            let mut alert_id = None;
            let mut user = None;

            entry.incident.as_ref().map(|inc| {
                inc.summary.as_ref().map(|summary| {
                    let parts: Vec<&str> = summary.split('-').collect();

                    parts.last().as_ref().map(|last| {
                        if last.starts_with("ID#") {
                            AlertId::from_str(last.replace("ID#", "").as_str()).map(|id| {
                                alert_id = Some(id);
                            });
                        }
                    });
                });
            });

            entry.agent.as_ref().map(|agent| {
                agent.summary.as_ref().map(|summary| {
                    user = Some(User::PagerDuty(summary.to_string()));
                })
            });

            if alert_id.is_none() || user.is_none() {
                continue;
            }

            acks.push((alert_id.unwrap(), user.unwrap()));
        }

        acks
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LogEntry {
    #[serde(rename = "type")]
    ty: Option<String>,
    agent: Option<LogAgent>,
    incident: Option<LogEntryIncident>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LogAgent {
    summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LogEntryIncident {
    summary: Option<String>,
}

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

async fn auth_post<T: Serialize, R: DeserializeOwned>(
    url: &str,
    client: &reqwest::Client,
    api_key: &str,
    data: &T,
) -> Result<R> {
    let resp = client
        .post(url)
        .header(AUTHORIZATION, api_key)
        .json(data)
        .send()
        .await?;

    resp.json::<R>().await.map_err(|err| err.into())
}

async fn auth_get<R: DeserializeOwned>(
    url: &str,
    client: &reqwest::Client,
    api_key: &str,
) -> Result<R> {
    let resp = client
        .get(url)
        .header(AUTHORIZATION, api_key)
        .send()
        .await?;

    resp.json::<R>().await.map_err(|err| err.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        primitives::{Alert, AlertContext},
        unix_time,
    };
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

        let _resp = client.handle(notification).await.unwrap();
    }

    #[ignore]
    #[tokio::test]
    async fn fetch_log_entries() {
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
        client.fetch_log_entries().await;
    }
}
