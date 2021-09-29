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
use structopt::StructOpt;

mod database;
mod matrix;
mod processor;
mod webhook;

pub type Result<T> = std::result::Result<T, anyhow::Error>;

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct AlertId(u64);

impl AlertId {
    pub fn from_le_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 8 {
            return Err(anyhow!("failed to fetch current id, invalid length"));
        }

        let mut id = [0; 8];
        id.copy_from_slice(&bytes);
        Ok(AlertId(u64::from_le_bytes(id)))
    }
    pub fn from_str(str: &str) -> Result<Self> {
        Ok(AlertId(str.parse()?))
    }
    pub fn incr(self) -> Self {
        AlertId(self.0 + 1)
    }
    pub fn to_le_bytes(self) -> [u8; 8] {
        self.0.to_le_bytes()
    }
    pub fn inner(&self) -> u64 {
        self.0
    }
}

impl From<u64> for AlertId {
    fn from(val: u64) -> Self {
        AlertId(val)
    }
}

impl ToString for AlertId {
    fn to_string(&self) -> String {
        self.0.to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    db_path: String,
    matrix: matrix::MatrixConfig,
    listener: String,
    escalation_window: u64,
    rooms: Vec<String>,
}

#[derive(StructOpt, Debug)]
#[structopt(name = "matrixbot")]
struct Cli {
    #[structopt(short, long)]
    config: String,
}

pub async fn run() -> Result<()> {
    let cli = Cli::from_args();

    // TODO: Set via config
    let should_escalate = true;

    env_logger::builder()
        .filter_module("system", log::LevelFilter::Debug)
        .init();

    info!("Logger initialized");

    info!(
        "Opening config at {}",
        std::fs::canonicalize(&cli.config)?
            .to_str()
            .ok_or(anyhow!("Path to config is not valid unicode"))?
    );
    let content = std::fs::read_to_string(&cli.config)?;
    let config: Config = serde_yaml::from_str(&content)?;

    if config.rooms.is_empty() {
        return Err(anyhow!("No alert rooms have been configured"));
    }

    info!("Setting up database {}", &config.db_path);
    // TODO:
    let db = database::Database::new("", "").await?;

    info!("Adding message processor to system registry");
    let proc = processor::Processor::new(db, config.escalation_window, should_escalate);
    SystemRegistry::set(proc.start());

    info!("Initializing Matrix client");
    let matrix = matrix::MatrixClient::new(&config.matrix, config.rooms).await?;

    SystemRegistry::set(matrix.start());

    info!("Starting API server");
    webhook::run_api_server(&config.listener).await?;

    loop {
        sleep(Duration::from_secs(u64::MAX)).await;
    }
}
