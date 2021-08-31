use crate::webhook::Alert;
use crate::{AlertId, Result};
use crate::processor::AlertContext;
use rocksdb::{ColumnFamilyDescriptor, Options, DB};
use std::collections::HashMap;

const PENDING: &'static str = "pending_alerts";

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

        Ok(Database { db: db })
    }
    pub fn insert_alerts(&self, alerts: Vec<Alert>) -> Result<()> {
        let cf = self.db.cf_handle("PENDING").unwrap();

        for alert in alerts {
            self.db.put_cf(&cf, AlertId::new(), AlertContext::new(alert).to_bytes().as_slice());
        }

        Ok(())
    }
    pub fn acknowledge_alert(&self, id: &AlertId) {
        let cf = self.db.cf_handle("PENDING").unwrap();
    }
}
