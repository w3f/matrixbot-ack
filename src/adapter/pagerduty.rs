use crate::processor::NotifyAlert;
use crate::AlertId;
use crate::Result;
use actix::prelude::*;
use actix::SystemService;
use reqwest::header::AUTHORIZATION;
use reqwest::StatusCode;
use tokio::time::{sleep, Duration};

const SEND_ALERT_ENDPOINT: &str = "https://events.pagerduty.com/v2/enqueue";
const RETRY_TIMEOUT: u64 = 10; // seconds

/// Alert Event for the PagerDuty API.
#[derive(Debug, Clone, Serialize)]
pub struct AlertEvent {
    #[serde(skip_serializing)]
    id: AlertId,
    routing_key: String,
    event_action: EventAction,
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
pub enum EventAction {
    Trigger,
    Acknowledge,
    Resolve,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PayloadSeverity {
    Critical,
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceConfig {
    api_key: String,
    integration_key: String,
    event_action: EventAction,
    payload_source: String,
    payload_severity: PayloadSeverity,
}

pub struct PagerDutyClient {
    config: ServiceConfig,
}

impl PagerDutyClient {
    pub fn new(mut config: ServiceConfig) -> Self {
        config.api_key = format!("Token token={}", config.api_key);

        PagerDutyClient { config }
    }
}

fn new_alert_events(config: &ServiceConfig, notify: &NotifyAlert) -> Vec<AlertEvent> {
    let mut alerts = vec![];

    for alert in &notify.alerts {
        alerts.push(AlertEvent {
            id: alert.id,
            routing_key: config.integration_key.clone(),
            event_action: config.event_action,
            payload: Payload {
                summary: alert.to_string(),
                source: config.payload_source.clone(),
                severity: config.payload_severity,
            },
        });
    }

    alerts
}

impl Default for PagerDutyClient {
    fn default() -> Self {
        panic!("PagerDuty client was not initialized in the system registry. This is a bug.");
    }
}

impl Actor for PagerDutyClient {
    type Context = Context<Self>;
}

impl SystemService for PagerDutyClient {}
impl Supervised for PagerDutyClient {}

impl Handler<NotifyAlert> for PagerDutyClient {
    type Result = ResponseActFuture<Self, Result<()>>;

    fn handle(&mut self, notify: NotifyAlert, _ctx: &mut Self::Context) -> Self::Result {
        let config = self.config.clone();

        let f = async move {
            if notify.alerts.is_empty() {
                return Ok(());
            }

            let client = reqwest::Client::new();

            for alert in new_alert_events(&config, &notify) {
                loop {
                    let resp = client
                        .post(SEND_ALERT_ENDPOINT)
                        .header(AUTHORIZATION, &config.api_key)
                        .json(&alert)
                        .send()
                        .await?;

                    match resp.status() {
                        StatusCode::ACCEPTED => {
                            info!("Submitted alert {} to PagerDuty", alert.id);
                            break;
                        }
                        err => error!(
                            "Failed to send alert to PagerDuty: {:?}, response: {:?}",
                            err, resp
                        ),
                    }

                    warn!("Retrying...");
                    sleep(Duration::from_secs(RETRY_TIMEOUT)).await;
                }
            }

            Ok(())
        };

        Box::pin(f.into_actor(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processor::AlertContext;
    use actix::SystemRegistry;
    use std::env;

    #[ignore]
    #[actix_web::test]
    async fn submit_alert_event() {
        let integration_key = env::var("PD_SERVICE_KEY").unwrap();
        let api_key = env::var("PD_API_KEY").unwrap();

        let config = ServiceConfig {
            api_key,
            integration_key,
            event_action: EventAction::Trigger,
            payload_source: "matrixbot-ack-test".to_string(),
            payload_severity: PayloadSeverity::Warning,
        };

        let client = PagerDutyClient::new(config);
        SystemRegistry::set(client.start());

        let alerts = vec![
            AlertContext::new_test(0, "First alert"),
            AlertContext::new_test(0, "Second alert"),
        ];

        let _ = PagerDutyClient::from_registry()
            .send(NotifyAlert { alerts })
            .await
            .unwrap()
            .unwrap();
    }
}
