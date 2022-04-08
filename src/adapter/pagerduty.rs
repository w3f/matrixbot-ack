use crate::Result;
use crate::processor::NotifyAlert;
use actix::prelude::*;
use actix::SystemService;

/// Alert Event for the PagerDuty API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertEvent {
	routing_key: String,
	event_action: EventAction,
	#[serde(rename = "payload.summary")]
	payload_summary: String,
	#[serde(rename = "payload.source")]
	payload_source: String,
	#[serde(rename = "payload.severity")]
	payload_severity: PayloadSeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventAction {
	Trigger,
	Acknowledge,
	Resolve,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PayloadSeverity {
	Critical,
	Error,
	Warning,
	Info,
}

pub struct PagerDutyClient {

}

impl PagerDutyClient {

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
		let f = async move {
            if notify.alerts.is_empty() {
                return Ok(());
            }

			unimplemented!();
		};

		Box::pin(f.into_actor(self))
	}
}