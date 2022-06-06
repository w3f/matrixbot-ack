use crate::database::Database;
use crate::processor::{InsertAlerts, Processor};
use crate::Result;
use actix::prelude::*;
use actix_web::{web, App, HttpResponse, HttpServer};
use actix_broker::{Broker, BrokerIssue, SystemBroker};

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Alert {
    pub annotations: Annotations,
    pub labels: Labels,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Annotations {
    pub message: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Labels {
    pub severity: String,
    #[serde(rename = "alertname")]
    pub alert_name: String,
}

pub async fn run_api_server(endpoint: &str, db: Database) -> Result<()> {
    let server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(db.clone()))
            .route("/healthcheck", web::get().to(healthcheck))
            .route("/webhook-ack", web::post().to(insert_alerts))
    })
    .bind(endpoint)?;

    let _ = server.run();
    Ok(())
}

async fn healthcheck() -> HttpResponse {
    HttpResponse::Ok().body("OK")
}

async fn insert_alerts(req: web::Json<InsertAlerts>, db: web::Data<Database>) -> HttpResponse {
    let alerts = req.into_inner();
    debug!("New alerts received from webhook: {:?}", alerts);

    // Attempt to insert the events into the database.
    match db.insert_alerts(&alerts).await {
        Ok(_) => {
            // Notify broker about new alerts.
            Broker::<SystemBroker>::issue_async(alerts);

            HttpResponse::Ok().body("OK")
        },
        Err(err) => {
            error!("Failed to process new alerts: {:?}", err);
            HttpResponse::InternalServerError().finish()
        }
    }
}
