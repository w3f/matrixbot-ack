use crate::database::Database;
use crate::webhook::Alert;
use crate::{AlertId, Result};
use actix::prelude::*;

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct AlertContext {
    id: AlertId,
    alert: Alert,
    escalation_idx: usize,
}

impl AlertContext {
    pub fn new(alert: Alert) -> Self {
        AlertContext {
            id: AlertId::new(),
            alert: alert,
            escalation_idx: 0,
        }
    }
    pub fn from_bytes(slice: &[u8]) -> Result<Self> {
        serde_json::from_slice(slice).map_err(|err| err.into())
    }
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
}

pub struct Processor {
    db: Database,
}

impl Default for Processor {
    fn default() -> Self {
        panic!("Processor was not initialized");
    }
}

impl Actor for Processor {
    type Context = Context<Self>;
}

impl SystemService for Processor {}
impl Supervised for Processor {}

#[derive(Clone, Debug, Eq, PartialEq, Message)]
#[rtype(result = "UserConfirmation")]
pub enum UserAction {
    Ack(AlertId),
    Pending,
    Help,
}

pub enum UserConfirmation {
    InternalError,
}

#[derive(Clone, Debug, Eq, PartialEq, Message)]
#[rtype(result = "()")]
pub struct InsertAlerts {
    alerts: Vec<Alert>,
}

impl Handler<UserAction> for Processor {
    type Result = ResponseActFuture<Self, UserConfirmation>;

    fn handle(&mut self, msg: UserAction, _ctx: &mut Self::Context) -> Self::Result {
        unimplemented!()
        /*
        match msg {
            UserAction::Ack(id) => {

            }
            UserAction::Pending => {

            }
            UserAction::Help => {

            }
        }
        */
    }
}

impl Handler<InsertAlerts> for Processor {
    type Result = ResponseActFuture<Self, ()>;

    fn handle(&mut self, _msg: InsertAlerts, _ctx: &mut Self::Context) -> Self::Result {
        let f = async move {};

        Box::pin(f);

        unimplemented!()
    }
}
