use crate::adapter::matrix::MatrixClient;
use crate::adapter::pagerduty::PagerDutyClient;
use crate::database::Database;
use crate::webhook::Alert;
use crate::{unix_time, AlertId, Result};
use actix::prelude::*;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

const CRON_JOB_INTERVAL: u64 = 5;

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct AlertContext {
    pub id: AlertId,
    pub alert: Alert,
    pub escalation_idx: usize,
    pub last_notified: u64,
    pub should_escalate: bool,
}

impl AlertContext {
    pub fn new(alert: Alert, id: AlertId, should_escalate: bool) -> Self {
        AlertContext {
            id,
            alert,
            escalation_idx: 0,
            last_notified: unix_time(),
            should_escalate,
        }
    }
    pub fn should_escalate(&self) -> bool {
        self.should_escalate
    }
}

/// A trimmed version of `AlertContext`. Used when an alert should not escalate
/// (i.e. incoming transactions).
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AlertContextTrimmed(Alert);

impl From<AlertContext> for AlertContextTrimmed {
    fn from(val: AlertContext) -> Self {
        AlertContextTrimmed(val.alert)
    }
}

impl ToString for AlertContextTrimmed {
    fn to_string(&self) -> String {
        format!(
            "\
            - Name: {}\n  \
              Severity: {}\n  \
              Message: {}\n  \
              Description: {}\n\
        ",
            self.0.labels.alert_name,
            self.0.labels.severity,
            self.0.annotations.message.as_deref().unwrap_or("N/A"),
            self.0.annotations.description.as_deref().unwrap_or("N/A")
        )
    }
}

impl ToString for AlertContext {
    fn to_string(&self) -> String {
        format!(
            "\
            - ID: {}\n  \
              Name: {}\n  \
              Severity: {}\n  \
              Message: {}\n  \
              Description: {}\n\
        ",
            self.id,
            self.alert.labels.alert_name,
            self.alert.labels.severity,
            self.alert.annotations.message.as_deref().unwrap_or("N/A"),
            self.alert
                .annotations
                .description
                .as_deref()
                .unwrap_or("N/A")
        )
    }
}

pub struct Processor {
    db: Option<Arc<Database>>,
    escalation_window: u64,
    should_escalate: bool,
    // Ensures that only one escalation task is running at the time.
    escalation_lock: Arc<Mutex<()>>,
    pager_duty_enabled: bool,
}

impl Processor {
    pub fn new(
        db: Option<Database>,
        escalation_window: u64,
        should_escalate: bool,
        pager_duty_enabled: bool,
    ) -> Self {
        Processor {
            db: db.map(Arc::new),
            escalation_window,
            should_escalate,
            escalation_lock: Default::default(),
            pager_duty_enabled,
        }
    }
    fn db(&self) -> Arc<Database> {
        Arc::clone(self.db.as_ref().expect("Database has not been configured"))
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
        if self.should_escalate {
            let db = self.db();
            let escalation_window = self.escalation_window;

            let local = |db: Arc<Database>, escalation_window: u64| async move {
                let mut pending = db.get_pending(Some(escalation_window)).await?;

                for alert in &mut pending {
                    debug!("Alert escalated: {:?}", alert);

                    // Send alert to the matrix client, increment escalation index.
                    let is_last = MatrixClient::from_registry()
                        .send(Escalation {
                            escalation_idx: alert.escalation_idx + 1,
                            alerts: vec![alert.clone()],
                        })
                        .await??;

                    // Update alert info.
                    if !is_last {
                        alert.escalation_idx += 1;
                    }
                    alert.last_notified = unix_time();
                }

                // Update all alert states.
                db.insert_alerts(&pending).await?;

                Result::<()>::Ok(())
            };

            let lock = Arc::clone(&self.escalation_lock);
            ctx.run_interval(
                Duration::from_secs(CRON_JOB_INTERVAL),
                move |_proc, _ctx| {
                    // Acquire new handles for async task.
                    let db = Arc::clone(&db);
                    let lock = Arc::clone(&lock);

                    actix::spawn(async move {
                        // Immediately exits if the lock cannot be acquired.
                        if let Ok(locked) = lock.try_lock() {
                            // Lock acquired and will remain locked until
                            // `_l` goes out of scope.
                            let _l = locked;

                            match local(db, escalation_window).await {
                                Ok(_) => {}
                                Err(err) => error!("{:?}", err),
                            }
                        }
                    });
                },
            );
        }
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
    Ack(AlertId, String),
    Pending,
    Help,
}

#[derive(Clone, Debug, Eq, PartialEq, Message)]
#[rtype(result = "Result<()>")]
pub struct NotifyAlert {
    pub alerts: Vec<AlertContext>,
}

#[derive(Clone, Debug, Eq, PartialEq, Message)]
#[rtype(result = "Result<bool>")]
pub struct Escalation {
    pub escalation_idx: usize,
    pub alerts: Vec<AlertContext>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "Result<()>")]
pub struct InsertAlerts {
    alerts: Vec<Alert>,
}

impl Handler<UserAction> for Processor {
    type Result = ResponseActFuture<Self, UserConfirmation>;

    fn handle(&mut self, msg: UserAction, _ctx: &mut Self::Context) -> Self::Result {
        let db = self.db();

        let f = async move {
            async fn local(db: Arc<Database>, msg: UserAction) -> Result<UserConfirmation> {
                match msg.command {
                    Command::Ack(id, acked_by) => {
                        info!("Acknowledging alert Id: {}", id.to_string());
                        db.acknowledge_alert(msg.escalation_idx, id, acked_by).await
                    }
                    Command::Pending => db
                        .get_pending(None)
                        .await
                        .map(UserConfirmation::PendingAlerts),
                    Command::Help => Ok(UserConfirmation::Help),
                }
            }

            local(db, msg)
                .await
                .map_err(|err| {
                    error!("{:?}", err);
                    UserConfirmation::InternalError
                })
                .unwrap()
        };

        Box::pin(f.into_actor(self))
    }
}

impl Handler<InsertAlerts> for Processor {
    type Result = ResponseActFuture<Self, Result<()>>;

    fn handle(&mut self, msg: InsertAlerts, _ctx: &mut Self::Context) -> Self::Result {
        let db = self.db();
        let should_escalate = self.should_escalate;
        let pager_duty_enabled = self.pager_duty_enabled;

        let f = async move {
            // Convert webhook alerts into alert contexts.
            // (avoid an iterator so `async` can be used conveniently)
            let mut alerts = vec![];
            for alert in msg.alerts {
                let next_id = db.get_next_id().await?;
                alerts.push(AlertContext::new(alert, next_id, should_escalate));
            }

            // Only store alerts that should escalate.
            if should_escalate {
                db.insert_alerts(&alerts).await.map_err(|err| {
                    error!("Failed to insert alerts into database: {:?}", err);
                    err
                })?;
            }

            // Notify rooms about all alerts.
            debug!("Notifying rooms about new alerts");
            let _ = MatrixClient::from_registry()
                .send(NotifyAlert {
                    alerts: alerts.clone(),
                })
                .await??;

            if pager_duty_enabled {
                debug!("Notifying PagerDuty about new alerts");
                let _ = PagerDutyClient::from_registry()
                    .send(NotifyAlert { alerts })
                    .await??;
            }

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
            UserConfirmation::PendingAlerts(alerts) => {
                if alerts.is_empty() {
                    return "No pending alerts!".to_string()
                }

                let mut content = String::from("Pending alerts:\n");
                for alert in alerts {
                    content.push_str(&alert.to_string());
                }

                content
            }
            UserConfirmation::AlertOutOfScope => {
                "The alert has already reached the next escalation level. It cannot be acknowledged!".to_string()
            }
            UserConfirmation::AlertAcknowledged(id) => {
                format!("Alert {} has been acknowledged.", id)
            }
            UserConfirmation::AlertNotFound => {
                "The alert Id has not been found!".to_string()
            }
            UserConfirmation::Help => {
                "ack <ID> - Acknowledge an alert by id\npending - Show pending alerts\nhelp - Show this help message".to_string()
            }
            UserConfirmation::InternalError => {
                "There was an internal error. Please contact the admin.".to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::webhook::{Alert, Annotations, Labels};

    impl AlertContext {
        pub fn new_test(id: u64, msg: &str) -> Self {
            AlertContext {
                id: AlertId(id),
                alert: Alert {
                    annotations: Annotations {
                        message: Some(msg.to_string()),
                        description: Some("Test message".to_string()),
                    },
                    labels: Labels {
                        severity: "N/A".to_string(),
                        alert_name: "N/A".to_string(),
                    },
                },
                escalation_idx: 0,
                last_notified: 0,
                should_escalate: false,
            }
        }
    }
}
