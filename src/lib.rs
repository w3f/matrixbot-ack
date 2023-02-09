#[macro_use]
extern crate log;
#[macro_use]
extern crate anyhow;
#[macro_use]
extern crate serde;

use actix::{prelude::*, SystemRegistry};
use structopt::StructOpt;

mod database;
mod matrix;
mod processor;
mod webhook;

pub type Result<T> = std::result::Result<T, anyhow::Error>;

const MIN_ESCALATION_WINDOW: u64 = 60; // 60 seconds

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct AlertId(u64);

impl AlertId {
    fn from_str(str: &str) -> Result<Self> {
        Ok(AlertId(str.parse()?))
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

fn unix_time() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    let start = SystemTime::now();
    start
        .duration_since(UNIX_EPOCH)
        .expect("Failed to calculate UNIX time")
        .as_secs()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    database: Option<database::DatabaseConfig>,
    matrix: matrix::MatrixConfig,
    listener: String,
    escalation: Option<EscalationConfig>,
    rooms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EscalationConfig {
    enabled: bool,
    escalation_window: u64,
}

#[derive(StructOpt, Debug)]
#[structopt(name = "matrixbot")]
struct Cli {
    #[structopt(short, long)]
    config: String,
}

pub async fn run() -> Result<()> {
    let cli = Cli::from_args();

    let mut builder = env_logger::builder();
    builder.filter_module("system", log::LevelFilter::Debug);

    println!(
        "Opening config at {}",
        std::fs::canonicalize(&cli.config)?
            .to_str()
            .ok_or_else(|| anyhow!("Path to config is not valid unicode"))?
    );

    let content = std::fs::read_to_string(&cli.config)?;
    let config: Config = serde_yaml::from_str(&content)?;

    // If enabled, display logs for matrix-sdk.
    if config.matrix.verbose_logs == Some(true) {
        builder.filter_module("matrix_sdk", log::LevelFilter::Debug);
    }

    builder.init();
    info!("Logger initialized");

    if config.rooms.is_empty() {
        return Err(anyhow!("No alert rooms have been configured"));
    }

    // Retrieve relevant escalation data.
    let should_escalate = config
        .escalation
        .as_ref()
        .map(|c| c.enabled)
        .unwrap_or(false);

    let escalation_window = config
        .escalation
        .as_ref()
        .map(|c| c.escalation_window)
        .unwrap_or(MIN_ESCALATION_WINDOW)
        .max(MIN_ESCALATION_WINDOW);

    if should_escalate && config.database.is_none() {
        return Err(anyhow!(
            "Escalations require a database configuration, which isn't provided"
        ));
    }

    let opt_db = if let Some(db_conf) = config.database {
        info!("Setting up database {:?}", db_conf);
        let db = database::Database::new(db_conf).await?;
        Some(db)
    } else {
        warn!("Skipping database setup");
        None
    };

    info!("Adding message processor to system registry");
    let proc = processor::Processor::new(opt_db, escalation_window, should_escalate);
    SystemRegistry::set(proc.start());

    info!("Initializing Matrix client");
    // Only handle user commands if escalations are enabled.
    let matrix = matrix::MatrixClient::new(&config.matrix, config.rooms, should_escalate).await?;

    SystemRegistry::set(matrix.start());

    info!("Starting API server");
    webhook::run_api_server(&config.listener)
        .await?
        .await
        .map_err(|err| err.into())
}
