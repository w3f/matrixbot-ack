use crate::primitives::{AlertContext, AlertId, PendingAlerts};
use crate::primitives::{NotifyNewlyInserted, User};
use crate::webhook::InsertAlerts;
use crate::{unix_time, Result};
use bson::{doc, to_bson};
use futures::StreamExt;
use mongodb::options::{FindOneAndUpdateOptions, ReturnDocument};
use mongodb::{Client, Database as MongoDb};
use std::time::Duration;

const PENDING: &str = "pending";
const _HISTORY: &str = "history";
const ID_CURSOR: &str = "id_cursor";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub uri: String,
    pub name: String,
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
        let now = unix_time();

        let res = pending
            .update_one(
                doc! {
                    "id": to_bson(&alert_id)?,
                },
                doc! {
                    "$set": {
                        "acked_by": to_bson(&acked_by)?,
                        "acked_at_tmsp": to_bson(&now)?,
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
    pub async fn get_pending(&self, interval: Option<Duration>) -> Result<PendingAlerts> {
        const INSERTED_OFFSET: u64 = 10;

        let pending = self.db.collection::<AlertContext>(PENDING);
        let now = unix_time();

        let last_notified_query = match interval {
            Some(i) => {
                doc! {
                    "last_notified_tmsp": {
                        "$lt": to_bson(&(now - i.as_secs()))?
                    }
                }
            }
            None => doc! {},
        };

        pending
            .find(
                doc! {
                    "inserted_tmsp": {
                        "$lt": to_bson(&(now-INSERTED_OFFSET))?
                    },
                    "$or": [
                        {
                            "last_notified_tmsp": null
                        },
                        last_notified_query,
                    ],
                    "acked_by": null
                },
                None,
            )
            .await?
            .map(|res| res.map_err(|err| err.into()))
            .collect::<Vec<Result<AlertContext>>>()
            .await
            .into_iter()
            .collect::<Result<Vec<AlertContext>>>()
            .map(|alerts| PendingAlerts { alerts })
    }
    // TODO: Check for modified entries?
    pub async fn mark_delivered(&self, id: AlertId) -> Result<()> {
        let pending = self.db.collection::<AlertContext>(PENDING);
        let now = unix_time();

        pending
            .update_one(
                doc! {
                    "id": to_bson(&id)?
                },
                doc! {
                    "$set": {
                        "last_notified_tmsp": to_bson(&now)?,
                    }
                },
                None,
            )
            .await?;

        Ok(())
    }
    pub async fn increment_alert_state(&self, id: AlertId, new_idx: usize) -> Result<()> {
        let pending = self.db.collection::<AlertContext>(PENDING);
        let now = unix_time();

        pending
            .update_one(
                doc! {
                    "id": to_bson(&id)?
                },
                doc! {
                    "$set": {
                        "level_idx": to_bson(&new_idx)?,
                        "last_notified_tmsp": to_bson(&now)?
                    }
                },
                None,
            )
            .await?;

        Ok(())
    }
}
