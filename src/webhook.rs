use crate::processor::{InsertAlerts, Processor};
use crate::{AlertId, Result};
use actix::{prelude::*, SystemRegistry};
use actix_web::{web, App, Error as ActixError, HttpRequest, HttpResponse, HttpServer};

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Alert {
    annotations: Annotations,
    labels: Labels,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Annotations {
    message: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Labels {
    severity: String,
    #[serde(rename = "alertname")]
    alert_name: String,
}

pub async fn run_api_server(endpoint: &str) -> Result<()> {
    let server = HttpServer::new(move || {
        App::new()
            .route("/healthcheck", web::get().to(healthcheck))
            .route("/webhook-ack", web::post().to(insert_alerts))
    })
    .bind(endpoint)?;

    server.run();
    Ok(())
}

async fn healthcheck() -> HttpResponse {
    HttpResponse::Ok().body("OK")
}

async fn insert_alerts(req: web::Json<InsertAlerts>) -> HttpResponse {
    let res = Processor::from_registry()
        .send(req.into_inner())
        .await
        .unwrap();

    match res {
        Ok(_) => HttpResponse::Ok().body("OK"),
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}
