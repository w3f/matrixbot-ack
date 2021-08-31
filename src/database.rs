use crate::processor::{AlertContext, UserConfirmation};
use crate::webhook::Alert;
use crate::{AlertId, Result};
use rocksdb::{ColumnFamilyDescriptor, IteratorMode, Options, DB};
use std::collections::HashMap;

const PENDING: &'static str = "pending_alerts";
const HISTORY: &'static str = "history";

pub struct Database {
    db: DB,
}

#[derive(Serialize, Deserialize)]
struct PendingAlertsEntry(HashMap<AlertId, Alert>);

impl Database {
    pub fn new(path: &str) -> Result<Self> {
        let ops = Options::default();

        let mut db = DB::open_default(path)?;
        db.create_cf(PENDING, &ops);
        db.create_cf(HISTORY, &ops);

        Ok(Database { db: db })
    }
    pub fn insert_alerts(&self, alerts: Vec<Alert>) -> Result<()> {
        let pending = self.db.cf_handle(PENDING).unwrap();

        for alert in alerts {
            let alert = AlertContext::new(alert);

            self.db
                .put_cf(&pending, alert.id, alert.to_bytes().as_slice());
        }

        Ok(())
    }
    pub fn acknowledge_alert(
        &self,
        escalation_idx: usize,
        id: &AlertId,
    ) -> Result<UserConfirmation> {
        let pending = self.db.cf_handle(PENDING).unwrap();
        let history = self.db.cf_handle(HISTORY).unwrap();

        if let Some(alert) = self.db.get_cf(pending, id)? {
            let ctx = AlertContext::from_bytes(alert.as_slice())?;
            if ctx.escalation_idx > escalation_idx {
                return Ok(UserConfirmation::AlertOutOfScope);
            }

            self.db.put_cf(history, id, &alert)?;
            self.db.delete_cf(pending, id)?;

            Ok(UserConfirmation::AlertAcknowledged)
        } else {
            Ok(UserConfirmation::AlertNotFound)
        }
    }
    pub fn get_pending(&self) -> Result<Vec<AlertContext>> {
        let pending = self.db.cf_handle(PENDING).unwrap();

        let mut alerts = vec![];
        for (_, alert) in self.db.iterator_cf(pending, IteratorMode::Start) {
            alerts.push(AlertContext::from_bytes(&alert)?);
        }

        Ok(alerts)
    }
}
