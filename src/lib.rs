#[macro_use]
extern crate log;
#[macro_use]
extern crate anyhow;
#[macro_use]
extern crate serde;
#[macro_use]
extern crate async_trait;

use actix::clock::sleep;
use actix::{prelude::*, SystemRegistry};
use std::time::Duration;

mod database;
mod matrix;
mod processor;
mod webhook;

pub type Result<T> = std::result::Result<T, anyhow::Error>;

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct AlertId(uuid::Uuid);

impl AlertId {
    fn new() -> Self {
        AlertId(uuid::Uuid::new_v4())
    }
    fn parse_str(input: &str) -> Result<Self> {
        Ok(AlertId(uuid::Uuid::parse_str(input)?))
    }
}

impl ToString for AlertId {
    fn to_string(&self) -> String {
        self.0.to_string()
    }
}

impl AsRef<[u8]> for AlertId {
    fn as_ref(&self) -> &[u8] {
        &self.0.as_bytes()[..]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    db_path: String,
    matrix: MatrixConfig,
    listener: String,
    rooms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MatrixConfig {
    homeserver: String,
    username: String,
    password: String,
    db_path: String,
}

pub async fn run() -> Result<()> {
    env_logger::builder()
        .filter_module("system", log::LevelFilter::Debug)
        .init();

    info!("Logger initialized");

    info!("Opening config");
    let content = std::fs::read_to_string("config.yaml")?;
    let config: Config = serde_yaml::from_str(&content)?;

    if config.rooms.is_empty() {
        return Err(anyhow!("No alert rooms have been configured"));
    }

    info!("Setting up database");
    let db = database::Database::new(&config.db_path)?;

    info!("Adding message processor to system registry");
    let proc = processor::Processor::new(db);
    SystemRegistry::set(proc.start());

    info!("Initializing Matrix client");
    let matrix = matrix::MatrixClient::new(
        &config.matrix.homeserver,
        &config.matrix.username,
        &config.matrix.password,
        &config.matrix.db_path,
        config.rooms,
    )
    .await?;

    SystemRegistry::set(matrix.start());

    info!("Starting API server");
    webhook::run_api_server(&config.listener).await?;

    loop {
        sleep(Duration::from_secs(u64::MAX)).await;
    }
}
