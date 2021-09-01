use crate::database::Database;
use crate::matrix::MatrixClient;
use crate::webhook::Alert;
use crate::{AlertId, Result};
use actix::prelude::*;
use std::sync::Arc;
use std::time::Duration;

fn unix_time() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    let start = SystemTime::now();
    start
        .duration_since(UNIX_EPOCH)
        .expect("Failed to calculate UNIX time")
        .as_secs()
}

const ESCALATION_WINDOW: u64 = 60 * 60;
const CRON_JON_INTERVAL: u64 = 60;

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct AlertContext {
    pub id: AlertId,
    pub alert: Alert,
    pub escalation_idx: usize,
    pub last_notified: u64,
}

impl AlertContext {
    pub fn new(alert: Alert) -> Self {
        AlertContext {
            id: AlertId::new(),
            alert: alert,
            escalation_idx: 0,
            last_notified: unix_time(),
        }
    }
    pub fn from_bytes(slice: &[u8]) -> Result<Self> {
        serde_json::from_slice(slice).map_err(|err| err.into())
    }
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
}

impl ToString for AlertContext {
    fn to_string(&self) -> String {
        unimplemented!()
    }
}

pub struct Processor {
    db: Arc<Database>,
}

impl Processor {
    pub fn new(db: Database) -> Self {
        Processor { db: Arc::new(db) }
    }
}

impl Default for Processor {
    fn default() -> Self {
        panic!("Processor was not initialized in system registry. This is a bug.");
    }
}

impl Actor for Processor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let t_db = Arc::clone(&self.db);

        ctx.run_interval(
            Duration::from_secs(CRON_JON_INTERVAL),
            move |_proc, _ctx| {
                let db = Arc::clone(&t_db);
                futures::executor::block_on(async move {
                    let mut pending = db.get_pending().unwrap();

                    let now = unix_time();
                    for alert in &mut pending {
                        // If the escalation window of the alert is exceeded...
                        if now > alert.last_notified + ESCALATION_WINDOW {
                            // Send alert to the matrix client.
                            let new_idx = MatrixClient::from_registry()
                                .send(NotifyPending {
                                    escalation_idx: alert.escalation_idx + 1,
                                    alerts: vec![alert.clone()],
                                })
                                .await
                                .unwrap();

                            // Update escalation index.
                            alert.escalation_idx = new_idx.unwrap();
                            alert.last_notified = now;
                        }
                    }

                    // Update all alert states.
                    db.insert_alerts(&pending).unwrap();
                })
            },
        );
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

#[derive(Clone, Debug, Eq, PartialEq, Message)]
#[rtype(result = "Result<usize>")]
pub struct NotifyPending {
    pub escalation_idx: usize,
    pub alerts: Vec<AlertContext>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "Result<()>")]
pub struct InsertAlerts {
    alerts: Vec<Alert>,
}

impl From<InsertAlerts> for Vec<AlertContext> {
    fn from(val: InsertAlerts) -> Self {
        val.alerts
            .into_iter()
            .map(|alert| AlertContext::new(alert))
            .collect()
    }
}

impl Handler<UserAction> for Processor {
    type Result = MessageResult<UserAction>;

    fn handle(&mut self, msg: UserAction, _ctx: &mut Self::Context) -> Self::Result {
        fn local(proc: &Processor, msg: UserAction) -> Result<UserConfirmation> {
            match msg.command {
                Command::Ack(id) => {
                    info!("Acknowledging alert Id: {}", id.to_string());
                    proc.db.acknowledge_alert(msg.escalation_idx, &id)
                }
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
    type Result = ResponseActFuture<Self, Result<()>>;

    fn handle(&mut self, msg: InsertAlerts, _ctx: &mut Self::Context) -> Self::Result {
        let db = Arc::clone(&self.db);

        let f = async move {
            let alerts: Vec<AlertContext> = msg.into();

            // Store alerts in database.
            db.insert_alerts(&alerts).map_err(|err| {
                error!("Failed to insert alerts into database: {:?}", err);
                err
            })?;

            // Notify rooms.
            debug!("Notifying rooms about new alerts");
            let _ = MatrixClient::from_registry()
                .send(NotifyPending {
                    escalation_idx: 0,
                    alerts: alerts,
                })
                .await
                .unwrap();

            Ok(())
        };

        Box::pin(f.into_actor(self))
    }
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

impl ToString for UserConfirmation {
    fn to_string(&self) -> String {
        match self {
            UserConfirmation::PendingAlerts(_) => {

            }
            _ => {}
        }

        unimplemented!()
    }
}
