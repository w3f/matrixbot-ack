use super::{Adapter, LevelManager};
use crate::primitives::{
    AlertContext, AlertId, Command, Notification, User, UserAction, UserConfirmation,
};
use crate::Result;
use cached::{Cached, TimedCache};
use reqwest::header::AUTHORIZATION;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

use super::AdapterName;

const SEND_ALERT_ENDPOINT: &str = "https://events.pagerduty.com/v2/enqueue";
// TODO: Add note about limit
const GET_LOG_ENTRIES_ENDPOINT: &str = "https://api.pagerduty.com/log_entries?limit=20";
const FETCH_LOG_ENTRIES_INTERVAL: u64 = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PagerDutyConfig {
    api_key: String,
    payload_source: String,
    only_on_escalation: bool,
}

pub struct PagerDutyClient {
    levels: LevelManager<PagerDutyLevel>,
    config: PagerDutyConfig,
    client: Arc<reqwest::Client>,
    user_actions: Arc<Mutex<UnboundedReceiver<UserAction>>>,
    tx: Arc<UnboundedSender<UserAction>>,
}

#[async_trait]
impl Adapter for PagerDutyClient {
    fn name(&self) -> AdapterName {
        AdapterName::PagerDuty
    }
    async fn notify(&self, notification: Notification, level_idx: usize) -> Result<()> {
        if self.config.only_on_escalation && level_idx == 0 {
            // Do nothing on initial alert.
            return Ok(())
        } else {
            self.handle(notification).await
        }
    }
    async fn respond(&self, _: UserConfirmation, _level_idx: usize) -> Result<()> {
        // Ignored, no direct responses on PagerDuty.
        Ok(())
    }
    async fn endpoint_request(&self) -> Option<UserAction> {
        let mut l = self.user_actions.lock().await;
        l.recv().await
    }
}

impl PagerDutyClient {
    pub async fn new(mut config: PagerDutyConfig, levels: Vec<PagerDutyLevel>) -> Self {
        config.api_key = format!("Token token={}", config.api_key);

        let (tx, user_actions) = unbounded_channel();

        let client = PagerDutyClient {
            levels: LevelManager::from(levels),
            config,
            client: Arc::new(reqwest::Client::new()),
            user_actions: Arc::new(Mutex::new(user_actions)),
            tx: Arc::new(tx),
        };

        client.run_log_entries().await;

        client
    }
    #[rustfmt::skip]
    async fn handle(&self, notification: Notification) -> Result<()> {
        // Create an alert type native to the PagerDuty API.
        match notification {
            Notification::Alert { context: alert } => {
                // Don't create duplicate entries on PagerDuty. It can already
                // handle escalations.
                if alert.has_entry(self.name()) {
                    return Ok(())
                }

                let level = self
                    .levels
                    .single_level(alert.level_idx(self.name()));

                let alert = new_alert_event(
                    level.integration_key.to_string(),
                    self.config.payload_source.to_string(),
                    level.payload_severity,
                    &alert,
                );

                // Send authenticated POST request. We don't care about the
                // return value as long as it succeeds.
                let _resp = auth_post::<_, serde_json::Value>(
                    SEND_ALERT_ENDPOINT,
                    &self.client,
                    &self.config.api_key,
                    &alert
                ).await?;
            }
            Notification::Acknowledged { id: alert_id, acked_by: _, acked_on: _ } => {
                // NOTE: Acknowlegement of alerts always happens on the first
                // specified integration key.
                let level = self.levels.single_level(0);
                let ack = new_alert_ack(level.integration_key.to_string(), alert_id);

                // Send authenticated POST request. We don't care about the
                // return value as long as it succeeds.
                let _resp = auth_post::<_, serde_json::Value>(
                    SEND_ALERT_ENDPOINT,
                    &self.client,
                    &self.config.api_key,
                    &ack
                ).await?;
            }
        }

        // Ignore any other type of Notification

        Ok(())
    }
    async fn run_log_entries(&self) {
        let client = Arc::clone(&self.client);
        let tx = Arc::clone(&self.tx);
        let api_key = self.config.api_key.to_string();
        // Timed cache for alert Ids.
        let mut cache: TimedCache<AlertId, ()> = TimedCache::with_lifespan_and_refresh(3600, true);

        tokio::spawn(async move {
            loop {
                match auth_get::<LogEntries>(GET_LOG_ENTRIES_ENDPOINT, &client, &api_key).await {
                    Ok(entries) => {
                        for (alert_id, user) in entries.get_resolved() {
                            // Only create user action if the Id was not cached yet.
                            if cache.cache_set(alert_id, ()).is_none() {
                                debug!("New resolved entry detected by {}: {}", user, alert_id);

                                // Create user action
                                tx.send(UserAction {
                                    user,
                                    // Any PagerDuty level is the "last channel".
                                    channel_id: 0,
                                    is_last_channel: true,
                                    command: Command::Ack(alert_id),
                                })
                                .unwrap()
                            }
                        }
                    }
                    Err(err) => {
                        error!("Failed to fetch PagerDuty log entries: {:?}", err);
                    }
                }

                sleep(Duration::from_secs(FETCH_LOG_ENTRIES_INTERVAL)).await;
            }
        });
    }
}

/// Alert Event for the PagerDuty API.
#[derive(Debug, Clone, Serialize)]
pub struct AlertEvent {
    routing_key: String,
    event_action: EventAction,
    dedup_key: String,
    payload: Option<Payload>,
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
    fn get_resolved(&self) -> Vec<(AlertId, User)> {
        let entries: Vec<&LogEntry> = self
            .log_entries
            .iter()
            // Filter for acknowledged alerts
            .filter(|entry| {
                entry
                    .ty
                    .as_ref()
                    .map(|ty| ty.contains("resolve_log_entry"))
                    .unwrap_or(false)
            })
            .collect();

        let mut acks = vec![];

        for entry in entries {
            let mut alert_id = None;
            let mut user = None;

            if let Some(inc) = &entry.incident {
                if let Some(summary) = &inc.summary {
                    let parts: Vec<&str> = summary.split('-').collect();

                    if let Some(last) = parts.last().as_ref() {
                        let trimmed = last.trim();

                        if trimmed.starts_with("ID#") {
                            if let Ok(id) = AlertId::from_str(&trimmed.replace("ID#", "")) {
                                alert_id = Some(id);
                            };
                        }
                    };
                };
            };

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
        payload: Some(Payload {
            summary: alert.to_string_with_oneline(),
            source,
            severity,
        }),
    }
}

fn new_alert_ack(key: String, alert_id: AlertId) -> AlertEvent {
    AlertEvent {
        routing_key: key,
        event_action: EventAction::Acknowledge,
        dedup_key: format!("ID#{}", alert_id),
        payload: None,
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
            only_on_escalation: false,
        };

        let level = PagerDutyLevel {
            integration_key,
            payload_severity: PayloadSeverity::Warning,
        };

        let client = PagerDutyClient::new(config, vec![level]).await;

        let notification = Notification::Alert {
            context: AlertContext::new(unix_time().into(), Alert::new_test()),
        };

        let _resp = client.handle(notification).await.unwrap();
    }
}
