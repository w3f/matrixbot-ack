use crate::database::Database;
use crate::primitives::Alert;
use crate::Result;
use actix_web::{dev::Server, web, App, HttpResponse, HttpServer};

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct InsertAlerts {
    pub alerts: Vec<Alert>,
}

impl InsertAlerts {
    pub fn alerts_owned(self) -> Vec<Alert> {
        self.alerts
    }
}

pub async fn run_api_server(endpoint: &str, db: Database) -> Result<Server> {
    let server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(db.clone()))
            .route("/healthcheck", web::get().to(healthcheck))
            .route("/webhook-ack", web::post().to(insert_alerts))
    })
    .system_exit()
    .bind(endpoint)?;

    Ok(server.run())
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
        Ok(_) => HttpResponse::Ok().body("OK"),
        Err(err) => {
            error!("Failed to process new alerts: {:?}", err);
            HttpResponse::InternalServerError().finish()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl InsertAlerts {
        pub fn new_test() -> Self {
            InsertAlerts {
                alerts: vec![Alert::new_test()],
            }
        }
    }

    #[ignore]
    #[test]
    fn display_insert_test_alert() {
        let alerts = InsertAlerts::new_test();
        println!("{}", serde_json::to_string_pretty(&alerts).unwrap());
    }
}
