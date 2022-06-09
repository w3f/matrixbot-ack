use crate::primitives::{AlertContext, AlertId, PendingAlerts};
use crate::primitives::{NotifyNewlyInserted, User};
use crate::webhook::InsertAlerts;
use crate::Result;
use bson::{doc, to_bson};
use mongodb::options::{FindOneAndUpdateOptions, ReturnDocument};
use mongodb::{Client, Database as MongoDb};
use std::time::Duration;

const PENDING: &str = "pending";
const _HISTORY: &str = "history";
const ID_CURSOR: &str = "id_cursor";

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
    pub async fn insert_alerts(&self, inserts: InsertAlerts) -> Result<NotifyNewlyInserted> {
        let pending = self.db.collection::<AlertContext>(PENDING);

        let mut contexts = vec![];
        for alert in inserts.alerts_owned() {
            let ctx = AlertContext::new(self.get_next_id().await?, alert);

            // Insert the alert
            pending.insert_one(&ctx, None).await?;

            contexts.push(ctx);
        }

        Ok(NotifyNewlyInserted { alerts: contexts })
    }
    async fn get_next_id(&self) -> Result<AlertId> {
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
    }
    pub async fn acknowledge_alert(&self, alert_id: &AlertId, acked_by: &User) -> Result<bool> {
        let pending = self.db.collection::<AlertContext>(PENDING);

        let res = pending
            .update_one(
                doc! {
                    "id": to_bson(&alert_id)?,
                },
                doc! {
                    "$set": {
                        "acked_by": to_bson(&acked_by)?,
                    }
                },
                None,
            )
            .await?;

        if res.modified_count == 0 {
            Ok(false)
        } else {
            Ok(true)
        }
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
