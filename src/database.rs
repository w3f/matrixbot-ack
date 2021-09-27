use crate::processor::{AlertContext, UserConfirmation};
use crate::webhook::Alert;
use crate::{AlertId, Result};
use rocksdb::{IteratorMode, Options, DB};
use std::collections::HashMap;

const PENDING: &'static str = "pending_alerts";
const HISTORY: &'static str = "history";
const ID_CURSOR: &'static str = "id_cursor";

pub struct Database {
    db: DB,
}

#[derive(Serialize, Deserialize)]
struct PendingAlertsEntry(HashMap<AlertId, Alert>);

impl Database {
    pub fn new(path: &str) -> Result<Self> {
        let mut ops = Options::default();
        ops.create_if_missing(true);
        ops.create_missing_column_families(true);

        let db = DB::open_cf(&ops, path, [PENDING, HISTORY, ID_CURSOR])?;

        Ok(Database { db: db })
    }
    pub fn insert_alerts(&self, alerts: &[AlertContext]) -> Result<()> {
        let pending = self.db.cf_handle(PENDING).unwrap();
        let cursor = self.db.cf_handle(ID_CURSOR).unwrap();

        let mut id = AlertId::from(0);
        for alert in alerts {
            // Track the highest Id.
            id = id.max(alert.id);

            // Update Id cursor.
            self.db.put_cf(&cursor, ID_CURSOR, id.to_le_bytes())?;

            // Insert alert.
            self.db.put_cf(
                &pending,
                alert.id.to_le_bytes(),
                alert.to_bytes().as_slice(),
            )?;
        }

        Ok(())
    }
    pub fn get_next_id(&self) -> Result<AlertId> {
        let cursor = self.db.cf_handle(ID_CURSOR).unwrap();

        if let Some(id) = self.db.get_cf(cursor, ID_CURSOR)? {
            Ok(AlertId::from_le_bytes(&id)?.incr())
        } else {
            Ok(AlertId::from(0))
        }
    }
    pub fn acknowledge_alert(
        &self,
        escalation_idx: usize,
        alert_id: AlertId,
    ) -> Result<UserConfirmation> {
        let pending = self.db.cf_handle(PENDING).unwrap();
        let history = self.db.cf_handle(HISTORY).unwrap();
        let id = alert_id.to_le_bytes();

        if let Some(alert) = self.db.get_cf(pending, id)? {
            let ctx = AlertContext::from_bytes(alert.as_slice())?;
            if ctx.escalation_idx > escalation_idx {
                return Ok(UserConfirmation::AlertOutOfScope);
            }

            self.db.put_cf(history, id, &alert)?;
            self.db.delete_cf(pending, id)?;

            Ok(UserConfirmation::AlertAcknowledged(alert_id))
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
