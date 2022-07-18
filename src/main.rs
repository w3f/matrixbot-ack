use system::{run, Result};
use tracing::info;

#[actix_web::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_env_filter("system")
        .init();

    info!("Logger initialized");

    run().await
}
