use crate::adapter::AdapterName;
use crate::primitives::{AlertContext, AlertId, PendingAlerts, User};
use crate::webhook::InsertAlerts;
use crate::{unix_time, Result};
use bson::{doc, to_bson};
use futures::StreamExt;
use mongodb::options::{FindOneAndUpdateOptions, ReturnDocument, UpdateOptions};
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

pub enum AcknowlegementResult {
    Ok,
    OutOfScope,
    AlreadyAcknowleged(User),
    NotFound,
}

impl Database {
    pub async fn new(config: DatabaseConfig) -> Result<Self> {
        Ok(Database {
            db: Client::with_uri_str(config.uri)
                .await?
                .database(&config.name),
        })
    }
    pub async fn insert_alerts(&self, inserts: InsertAlerts) -> Result<()> {
        let pending = self.db.collection::<AlertContext>(PENDING);

        let mut contexts = vec![];
        for alert in inserts.alerts_owned() {
            let ctx = AlertContext::new(self.get_next_id().await?, alert);

            // Insert the alert
            pending.insert_one(&ctx, None).await?;

            contexts.push(ctx);
        }

        Ok(())
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
    pub async fn acknowledge_alert(
        &self,
        alert_id: &AlertId,
        acked_by: &User,
        adapter: AdapterName,
        level_idx: usize,
    ) -> Result<AcknowlegementResult> {
        let pending = self.db.collection::<AlertContext>(PENDING);
        let now = unix_time();

        let mut res = pending
            .find(
                doc! {
                    "id": to_bson(&alert_id)?,
                },
                None,
            )
            .await?;

        if let Some(doc) = res.next().await {
            let context = doc?;
            if context.level_idx(adapter) > level_idx {
                return Ok(AcknowlegementResult::OutOfScope);
            }

            if let Some(user) = context.acked_by {
                return Ok(AcknowlegementResult::AlreadyAcknowleged(user));
            }
        }

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
            Ok(AcknowlegementResult::NotFound)
        } else {
            Ok(AcknowlegementResult::Ok)
        }
    }
    pub async fn get_pending(
        &self,
        interval: Option<Duration>,
        adapter: Option<AdapterName>,
    ) -> Result<PendingAlerts> {
        let pending = self.db.collection::<AlertContext>(PENDING);
        let now = unix_time();

        // TODO: Test this individually.
        match (interval.is_some(), adapter.is_some()) {
            (true, false) | (false, true) => return Err(anyhow!("")),
            _ => {}
        }

        // Main query, search for unacknowleged alerts.
        let mut query = doc! {
            "acked_by": null,
        };

        if let Some(adapter) = adapter {
            let mut check_adapter = doc! {
                "adapters.name": to_bson(&adapter)?,
            };

            if let Some(interval) = interval {
                let limit = now - interval.as_secs();

                check_adapter.extend(doc! {
                    "adapters.last_notified_tmsp": {
                        "$lt": to_bson(&limit)?
                    }
                });
            }

            query.extend(doc! {
                "$or": [
                    check_adapter,
                    {
                        "adapters": {
                            "$not": {
                                "$elemMatch": {
                                    "name": to_bson(&adapter)?,
                                }
                            }
                        }
                    }
                ]
            });
        };

        pending
            .find(query, None)
            .await?
            .map(|res| res.map_err(|err| err.into()))
            .collect::<Vec<Result<AlertContext>>>()
            .await
            .into_iter()
            .collect::<Result<Vec<AlertContext>>>()
            .map(|alerts| PendingAlerts { alerts })
    }
    // TODO: Check for modified entries?
    pub async fn mark_delivered(&self, id: AlertId, adapter: AdapterName) -> Result<()> {
        let pending = self.db.collection::<AlertContext>(PENDING);
        let now = unix_time();

        pending
            .update_one(
                doc! {
                    "id": to_bson(&id)?,
                    "adapters": {
                        "$not": {
                            "$elemMatch": {
                                "name": to_bson(&adapter)?,
                            }
                        }
                    }
                },
                doc! {
                    "$push": {
                        "adapters": {
                            "name": to_bson(&adapter)?,
                            "level_idx": 0,
                        }
                    }
                },
                None,
            )
            .await?;

        pending
            .update_one(
                doc! {
                    "id": to_bson(&id)?,
                    "adapters.name": to_bson(&adapter)?,
                },
                doc! {
                    "$set": {
                        "adapters.$.last_notified_tmsp": to_bson(&now)?,
                    },
                    "$inc": {
                        "adapters.$.level_idx": 1,
                    }
                },
                None,
            )
            .await?;

        Ok(())
    }
    pub async fn get_level_idx(&self, adapter: AdapterName) -> Result<usize> {
        let pending = self.db.collection::<AlertContext>(PENDING);

        let mut res = pending
            .find(
                doc! {
                    "adapters.name": to_bson(&adapter)?,
                },
                None,
            )
            .await?;

        if let Some(doc) = res.next().await {
            let context = doc?;
            Ok(context.level_idx(adapter))
        } else {
            // Occurs if no adapter was registered for the alert, i.e. the alert
            // was created before the adapter was enabled.
            Ok(0)
        }
    }
}
