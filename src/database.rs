use crate::primitives::{AlertContext, AlertId, PendingAlerts, UserConfirmation};
use crate::primitives::{NotifyNewlyInserted, User};
use crate::webhook::InsertAlerts;
use crate::Result;
use bson::doc;
use mongodb::{Client, Database as MongoDb};
use std::time::Duration;

const PENDING: &str = "pending";
const _HISTORY: &str = "history";
const _ID_CURSOR: &str = "id_cursor";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    uri: String,
    name: String,
}

#[derive(Debug, Clone)]
pub struct Database {
    db: MongoDb,
}

#[derive(Debug, Serialize, Deserialize)]
struct IdCursor {
    latest_id: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct AlertAcknowledged {
    alert: AlertContext,
    acked_by: String,
    acked_timestamp: u64,
}

impl Database {
    pub async fn new(config: DatabaseConfig) -> Result<Self> {
        Ok(Database {
            db: Client::with_uri_str(config.uri)
                .await?
                .database(&config.name),
        })
    }
    pub async fn insert_alerts(&self, _inserts: InsertAlerts) -> Result<NotifyNewlyInserted> {
        let _pending = self.db.collection::<AlertContext>(PENDING);

        unimplemented!()
    }
    async fn _get_next_id(&self) -> Result<AlertId> {
        unimplemented!()
        /*
        let id_cursor = self.db.collection::<IdCursor>(ID_CURSOR);

        let id = id_cursor
            .find_one_and_update(
                doc! {},
                doc! {
                    "$inc": {
                        "latest_id": 1,
                    }
                },
                {
                    let mut ops = FindOneAndUpdateOptions::default();
                    // Return document *after* update.
                    ops.return_document = Some(ReturnDocument::After);
                    ops.upsert = Some(true);
                    Some(ops)
                },
            )
            .await?
            .map(|c| AlertId::from(c.latest_id))
            // Handled by `ReturnDocument::After`
            .unwrap();

        Ok(id)
         */
    }
    // TODO
    pub async fn ack(&self, _alert_id: &AlertId, _acked_by: &User) -> Result<UserConfirmation> {
        unimplemented!()
    }
    pub async fn get_pending(&self, _interval: Option<Duration>) -> Result<PendingAlerts> {
        unimplemented!()
    }
    pub async fn mark_delivered(&self, _alert: AlertId) -> Result<()> {
        unimplemented!()
    }
    // TODO: This should also mark as delivered.
    pub async fn increment_alert_state(&self, _alert: AlertId, _new_idx: usize) -> Result<()> {
        unimplemented!()
    }
}
