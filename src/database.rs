use crate::processor::{AlertContext, InsertAlerts, NotifyAlert, UserConfirmation};
use crate::webhook::Alert;
use crate::{unix_time, AlertId, Result};
use crate::primitives::User;
use bson::{doc, to_bson};
use futures::stream::StreamExt;
use mongodb::{
    options::{FindOneAndUpdateOptions, ReplaceOptions, ReturnDocument},
    Client, Database as MongoDb,
};
use std::collections::HashMap;

const PENDING: &str = "pending";
const HISTORY: &str = "history";
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

#[derive(Serialize, Deserialize)]
struct PendingAlertsEntry(HashMap<AlertId, Alert>);

impl Database {
    pub async fn new(config: DatabaseConfig) -> Result<Self> {
        Ok(Database {
            db: Client::with_uri_str(config.uri)
                .await?
                .database(&config.name),
        })
    }
    pub async fn insert_alerts(&self, alerts: InsertAlerts) -> Result<NotifyAlert> {
        if alerts.is_empty() {
            return Ok(());
        }

        let pending = self.db.collection::<AlertContext>(PENDING);

        // Insert the alerts themselves.
        for alert in alerts {
            let _ = pending
                .replace_one(
                    doc! {
                        "id": to_bson(&alert.id)?,
                    },
                    alert,
                    {
                        let mut ops = ReplaceOptions::default();
                        ops.upsert = Some(true);
                        ops
                    },
                )
                .await?;
        }

        unimplemented!()
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
    // TODO
    pub async fn ack(alert_id: AlertId, acked_by: User) -> Result<UserConfirmation> {
        unimplemented!()
    }
    pub async fn acknowledge_alert(
        &self,
        escalation_idx: usize,
        alert_id: AlertId,
        acked_by: String,
    ) -> Result<UserConfirmation> {
        let pending = self.db.collection::<AlertContext>(PENDING);
        let history = self.db.collection::<AlertAcknowledged>(HISTORY);

        let alert = pending
            .find_one(
                doc! {
                    "id": to_bson(&alert_id)?,
                },
                None,
            )
            .await?;

        if let Some(alert) = alert {
            if alert.escalation_idx <= escalation_idx {
                history
                    .insert_one(
                        AlertAcknowledged {
                            alert,
                            acked_by,
                            acked_timestamp: unix_time(),
                        },
                        None,
                    )
                    .await?;

                pending
                    .delete_one(
                        doc! {
                            "id": to_bson(&alert_id)?,
                        },
                        None,
                    )
                    .await?;

                Ok(UserConfirmation::AlertAcknowledged(alert_id))
            } else {
                Ok(UserConfirmation::AlertOutOfScope)
            }
        } else {
            Ok(UserConfirmation::AlertNotFound)
        }
    }
    pub async fn get_pending(&self, escalation_window: Option<u64>) -> Result<Vec<AlertContext>> {
        let pending = self.db.collection::<AlertContext>(PENDING);

        let query = if let Some(escalation_window) = escalation_window {
            let now = unix_time();
            doc! {
                "last_notified": {
                    "$lt": to_bson(&(now - escalation_window))?,
                }
            }
        } else {
            doc! {}
        };

        let mut cursor = pending.find(query, None).await?;

        let mut pending = vec![];
        while let Some(alert) = cursor.next().await {
            pending.push(alert?);
        }

        Ok(pending)
    }
}
