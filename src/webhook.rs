use crate::database::Database;
use crate::primitives::Alert;
use crate::Result;
use actix_broker::{Broker, SystemBroker};
use actix_web::{web, App, HttpResponse, HttpServer};

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct InsertAlerts {
    alerts: Vec<Alert>,
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
    let insert = req.into_inner();

    // Check if alerts are empty.
    if insert.alerts.is_empty() {
        return HttpResponse::Ok().body("OK");
    }

    debug!("New alerts received from webhook: {:?}", insert);

    // Attempt to insert the events into the database.
    match db.insert_alerts(insert).await {
        Ok(newly_inserted) => {
            // Notify broker about new alerts.
            Broker::<SystemBroker>::issue_async(newly_inserted);

            HttpResponse::Ok().body("OK")
        }
        Err(err) => {
            error!("Failed to process new alerts: {:?}", err);
            HttpResponse::InternalServerError().finish()
        }
    }
}
