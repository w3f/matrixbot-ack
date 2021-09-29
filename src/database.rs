use crate::processor::{AlertContext, UserConfirmation};
use crate::webhook::Alert;
use crate::{AlertId, Result};
// TODO: Can this be avoided somehow?
use bson::{doc, to_document};
use mongodb::{Client, Database as MongoDb};
use std::collections::HashMap;

const PENDING: &'static str = "pending_acknowledgement";
const ACKNOWLEDGED: &'static str = "history";
const ID_CURSOR: &'static str = "id_cursor";

pub struct Database {
    db: MongoDb,
}

#[derive(Debug, Serialize, Deserialize)]
struct IdCursor {
    last_id: u64,
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
    pub fn acknowledge_alert(
        &self,
        escalation_idx: usize,
        alert_id: AlertId,
    ) -> Result<UserConfirmation> {
        unimplemented!()

        /*
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
        */
    }
    pub fn get_pending(&self) -> Result<Vec<AlertContext>> {
        unimplemented!()

        /*
        let pending = self.db.cf_handle(PENDING).unwrap();

        let mut alerts = vec![];
        for (_, alert) in self.db.iterator_cf(pending, IteratorMode::Start) {
            alerts.push(AlertContext::from_bytes(&alert)?);
        }

        Ok(alerts)
        */
    }
}
