use crate::primitives::{Alert, AlertDelivery, AlertId, NotifyNewlyInserted};
use crate::Result;
use actix::prelude::*;
use actix::SystemService;
use actix_broker::BrokerSubscribe;
use reqwest::header::AUTHORIZATION;
use reqwest::StatusCode;
use tokio::time::{sleep, Duration};

const SEND_ALERT_ENDPOINT: &str = "https://events.pagerduty.com/v2/enqueue";
const RETRY_TIMEOUT: u64 = 10; // seconds

pub struct PagerDutyClient {
    config: PagerDutyConfig,
}

impl PagerDutyClient {
    pub fn new(mut config: PagerDutyConfig) -> Self {
        config.api_key = format!("Token token={}", config.api_key);

        PagerDutyClient { config }
    }
}

impl Actor for PagerDutyClient {
    type Context = Context<Self>;
}

impl Handler<AlertDelivery> for PagerDutyClient {
    type Result = ResponseActFuture<Self, Result<()>>;

    fn handle(&mut self, alert: AlertDelivery, _ctx: &mut Self::Context) -> Self::Result {
        let config = self.config.clone();

        let f = async move {
            let client = reqwest::Client::new();

            // Create an alert type native to the PagerDuty API.
            let alert = new_alert_event(
                "".to_string(),
                "".to_string(),
                config.payload_severity,
                &alert,
            );

            loop {
                let resp = post_alert(&client, config.api_key.as_str(), &alert)
                    .await
                    .unwrap();

                match resp.status() {
                    StatusCode::ACCEPTED => {
                        //info!("Submitted alert {} to PagerDuty", alert.id);
                        break;
                    }
                    StatusCode::BAD_REQUEST => {
                        //error!("BAD REQUEST when submitting alert {}", alert.id);
                        // Do not retry on this error type.
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

            Ok(())
        };

        Box::pin(f.into_actor(self))
    }
}

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
/// We only use Trigger.
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
pub struct PagerDutyConfig {
    api_key: String,
    integration_key: String,
    payload_source: String,
    payload_severity: PayloadSeverity,
}

fn new_alert_event(
    key: String,
    source: String,
    severity: PayloadSeverity,
    alert: &AlertDelivery,
) -> AlertEvent {
    AlertEvent {
        id: alert.id,
        routing_key: key.clone(),
        event_action: EventAction::Trigger,
        payload: Payload {
            summary: "TODO".to_string(),
            source: source.clone(),
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
    use crate::primitives::AlertContext;
    use actix::SystemRegistry;
    use std::env;

    #[ignore]
    #[actix_web::test]
    async fn submit_alert_event() {
        // Keep those entries a SECRET!
        let integration_key = env::var("PD_SERVICE_KEY").unwrap();
        let api_key = env::var("PD_API_KEY").unwrap();

        let config = PagerDutyConfig {
            api_key,
            integration_key,
            payload_source: "matrixbot-ack-test".to_string(),
            payload_severity: PayloadSeverity::Warning,
        };

        let client = PagerDutyClient::new(config);

        // TODO...
    }
}
