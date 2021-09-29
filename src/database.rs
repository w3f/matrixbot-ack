use crate::processor::{AlertContext, UserConfirmation};
use crate::webhook::Alert;
use crate::{unix_time, AlertId, Result};
// TODO: Can this be avoided somehow?
use bson::{doc, to_bson, to_document};
use futures::stream::StreamExt;
use mongodb::{Client, Database as MongoDb};
use std::collections::HashMap;

const PENDING: &'static str = "pending";
const HISTORY: &'static str = "history";
const ID_CURSOR: &'static str = "id_cursor";

pub struct Database {
    db: MongoDb,
}

#[derive(Debug, Serialize, Deserialize)]
struct IdCursor {
    last_id: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct AlertAcknowledged {
    alert: AlertContext,
    acked_by: String,
}

#[derive(Serialize, Deserialize)]
struct PendingAlertsEntry(HashMap<AlertId, Alert>);

impl Database {
    pub async fn new(uri: &str, db: &str) -> Result<Self> {
        Ok(Database {
            db: Client::with_uri_str(uri).await?.database(db),
        })
    }
    pub async fn insert_alerts(&self, alerts: &[AlertContext]) -> Result<()> {
        let id_cursor = self.db.collection::<IdCursor>(ID_CURSOR);
        let pending = self.db.collection::<AlertContext>(PENDING);

        // Find the highest
        let last_id = alerts
            .iter()
            .map(|alert| alert.id.inner())
            .max()
            .ok_or(anyhow!("no alerts specified"))?;

        let update = IdCursor { last_id: last_id };

        // Insert latest Id.
        let _ = id_cursor
            .update_one(doc! {}, to_document(&update)?, None)
            .await?;

        // Insert the alerts themselves.
        let _ = pending.insert_many(alerts.to_vec(), None).await?;

        Ok(())
    }
    pub async fn get_next_id(&self) -> Result<AlertId> {
        let id_cursor = self.db.collection::<IdCursor>(ID_CURSOR);

        let id = id_cursor
            .find_one(doc! {}, None)
            .await?
            .map(|c| AlertId::from(c.last_id).incr())
            .unwrap_or(AlertId::from(0));

        Ok(id)
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
            if alert.escalation_idx > escalation_idx {
                history
                    .insert_one(
                        AlertAcknowledged {
                            alert: alert,
                            acked_by: acked_by,
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
                    "$lt": now - escalation_window,
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
