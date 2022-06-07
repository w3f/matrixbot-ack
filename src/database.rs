use crate::primitives::User;
use crate::primitives::{AlertContext, AlertId, NotifyAlert, UserConfirmation};
use crate::webhook::InsertAlerts;
use crate::{unix_time, Result};
use bson::{doc, to_bson};
use futures::stream::StreamExt;
use mongodb::{
    options::{FindOneAndUpdateOptions, ReplaceOptions, ReturnDocument},
    Client, Database as MongoDb,
};
use std::collections::HashMap;
use std::time::Duration;

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

impl Database {
    pub async fn new(config: DatabaseConfig) -> Result<Self> {
        Ok(Database {
            db: Client::with_uri_str(config.uri)
                .await?
                .database(&config.name),
        })
    }
    pub async fn insert_alerts(&self, inserts: InsertAlerts) -> Result<NotifyAlert> {
        let pending = self.db.collection::<AlertContext>(PENDING);

        // Insert the alerts themselves.
        for alert in inserts.alerts() {
            /*
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
            */
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
    pub async fn ack(&self, alert_id: &AlertId, acked_by: &User) -> Result<UserConfirmation> {
        unimplemented!()
    }
    pub async fn acknowledge_alert(
        &self,
        escalation_idx: usize,
        alert_id: AlertId,
        acked_by: String,
    ) -> Result<UserConfirmation> {
        unimplemented!()
    }
    pub async fn get_pending(&self, interval: Option<Duration>) -> Result<NotifyAlert> {
        unimplemented!()
    }
    pub async fn update_pending(&self, alert: NotifyAlert) -> Result<NotifyAlert> {
        unimplemented!()
    }
}
