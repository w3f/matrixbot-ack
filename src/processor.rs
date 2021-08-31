use crate::database::Database;
use crate::webhook::Alert;
use crate::{AlertId, Result};
use actix::prelude::*;
use std::time::Duration;

const CRON_JON_INTERVAL: u64 = 60;

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct AlertContext {
    pub id: AlertId,
    pub alert: Alert,
    pub escalation_idx: usize,
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

impl Processor {
    pub fn new(db: Database) -> Self {
        Processor { db: db }
    }
}

impl Default for Processor {
    fn default() -> Self {
        panic!("Processor was not initialized");
    }
}

impl Actor for Processor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.run_interval(Duration::from_secs(CRON_JON_INTERVAL), |_proc, _ctx| {});
    }
}

impl SystemService for Processor {}
impl Supervised for Processor {}

#[derive(Clone, Debug, Eq, PartialEq, Message)]
#[rtype(result = "UserConfirmation")]
pub struct UserAction {
    pub escalation_idx: usize,
    pub command: Command,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    Ack(AlertId),
    Pending,
    Help,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UserConfirmation {
    PendingAlerts(Vec<AlertContext>),
    AlertOutOfScope,
    AlertAcknowledged(AlertId),
    AlertNotFound,
    Help,
    InternalError,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "Result<()>")]
pub struct InsertAlerts {
    alerts: Vec<Alert>,
}

impl Handler<UserAction> for Processor {
    type Result = MessageResult<UserAction>;

    fn handle(&mut self, msg: UserAction, _ctx: &mut Self::Context) -> Self::Result {
        fn local(proc: &Processor, msg: UserAction) -> Result<UserConfirmation> {
            match msg.command {
                Command::Ack(id) => proc.db.acknowledge_alert(msg.escalation_idx, &id),
                Command::Pending => proc
                    .db
                    .get_pending()
                    .map(|ctxs| UserConfirmation::PendingAlerts(ctxs)),
                Command::Help => Ok(UserConfirmation::Help),
            }
        }

        MessageResult(
            local(&self, msg)
                .map_err(|err| {
                    error!("{:?}", err);
                    UserConfirmation::InternalError
                })
                .unwrap(),
        )
    }
}

impl Handler<InsertAlerts> for Processor {
    type Result = Result<()>;

    fn handle(&mut self, msg: InsertAlerts, _ctx: &mut Self::Context) -> Self::Result {
        self.db.insert_alerts(msg.alerts).map_err(|err| {
            error!("Failed to insert alerts into database: {:?}", err);
            err
        })
    }
}
