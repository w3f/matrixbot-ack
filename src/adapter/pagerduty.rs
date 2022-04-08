use crate::Result;
use crate::processor::NotifyAlert;
use actix::prelude::*;
use actix::SystemService;

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
		unimplemented!()
	}
}