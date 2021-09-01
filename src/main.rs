use system::{run, Result};

#[actix_web::main]
async fn main() -> Result<()> {
    run().await
}
