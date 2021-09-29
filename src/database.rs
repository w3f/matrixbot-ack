use crate::processor::{AlertContext, UserConfirmation};
use crate::webhook::Alert;
use crate::{AlertId, Result};
// TODO: Can this be avoided somehow?
use bson::{doc, to_bson};
use mongodb::{Client, Database as MongoDb};
use std::collections::HashMap;

const PENDING: &'static str = "pending_acknowledgement";
const ACKNOWLEDGED: &'static str = "history";
const ID_CURSOR: &'static str = "id_cursor";

pub struct Database {
    db: MongoDb,
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
        let coll = self.db.collection::<AlertContext>(PENDING);

        // Find the highest
        let last_id = alerts
            .iter()
            .map(|alert| alert.id.inner())
            .max()
            .ok_or(anyhow!("no alerts specified"))?;

        // Insert latest Id.
        let _ = coll
            .update_one(
                doc! {},
                doc! {
                    "last_id": to_bson(&last_id)?,
                },
                None,
            )
            .await?;

        // Insert the alerts themselves.
        let _ = coll.insert_many(alerts.to_vec(), None).await?;

        Ok(())
    }
    pub fn get_next_id(&self) -> Result<AlertId> {
        unimplemented!()

        /*
        let cursor = self.db.cf_handle(ID_CURSOR).unwrap();

        if let Some(id) = self.db.get_cf(cursor, ID_CURSOR)? {
            Ok(AlertId::from_le_bytes(&id)?.incr())
        } else {
            Ok(AlertId::from(0))
        }
        */
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
