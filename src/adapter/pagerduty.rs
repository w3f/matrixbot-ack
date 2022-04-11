use crate::processor::AlertContextTrimmed;
use crate::processor::NotifyAlert;
use crate::AlertId;
use crate::Result;
use actix::prelude::*;
use actix::SystemService;
use reqwest::Client;

const SEND_ALERT_ENDPOINT: &'static str = "https://events.pagerduty.com/v2/enqueue";

/// Alert Event for the PagerDuty API.
#[derive(Debug, Clone, Serialize)]
pub struct AlertEvent {
    #[serde(skip_serializing)]
    id: AlertId,
    routing_key: String,
    event_action: EventAction,
    #[serde(rename = "payload.summary")]
    payload_summary: String,
    #[serde(rename = "payload.source")]
    payload_source: String,
    #[serde(rename = "payload.severity")]
    payload_severity: PayloadSeverity,
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
    integration_key: String,
    event_action: EventAction,
    payload_source: String,
    payload_severity: PayloadSeverity,
}

pub struct PagerDutyClient {
    config: ServiceConfig,
}

impl PagerDutyClient {
    pub fn new(config: ServiceConfig) -> Self {
        PagerDutyClient { config: config }
    }
}

fn new_alert_events(config: ServiceConfig, notify: &NotifyAlert) -> Vec<AlertEvent> {
    let mut alerts = vec![];

    for alert in &notify.alerts {
        alerts.push(AlertEvent {
            id: alert.id.clone(),
            routing_key: config.integration_key.clone(),
            event_action: config.event_action,
            payload_summary: AlertContextTrimmed::from(alert.clone()).to_string(),
            payload_source: config.payload_source.clone(),
            payload_severity: config.payload_severity,
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

            for alert in new_alert_events(config, &notify) {
                // TODO: Handle response.
                let res = client.post(SEND_ALERT_ENDPOINT).json(&alert).send().await?;
                println!(">> {:?}", res);
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

    #[ignore]
    #[actix_web::test]
    async fn submit_alert_event() {
        let integration_key = std::env::var("PD_INTEGRATION_KEY").unwrap();
        let config = vec![ServiceConfig {
            integration_key,
            event_action: EventAction::Acknowledge,
            payload_source: "matrixbot-ack-test".to_string(),
            payload_severity: PayloadSeverity::Warning,
        }];

        let client = PagerDutyClient::new(config);
        SystemRegistry::set(client.start());

        let alerts = vec![
            AlertContext::new_test(0, "First alert"),
            AlertContext::new_test(0, "Second alert"),
        ];

        let _ = PagerDutyClient::from_registry()
            .send(NotifyAlert { alerts: alerts })
            .await
            .unwrap()
            .unwrap();
    }
}
