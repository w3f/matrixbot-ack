#[macro_use]
extern crate tracing;
#[macro_use]
extern crate anyhow;
#[macro_use]
extern crate serde;
#[macro_use]
extern crate async_trait;

use adapter::matrix::MatrixConfig;
use adapter::pagerduty::{PagerDutyConfig, PayloadSeverity};
use database::DatabaseConfig;

use primitives::User;

use structopt::StructOpt;
use tokio::time::{sleep, Duration};

mod adapter;
mod database;
mod escalation;
mod primitives;
mod webhook;

pub type Result<T> = std::result::Result<T, anyhow::Error>;

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
    database: DatabaseConfig,
    listener: String,
    escalation: Option<EscalationConfig<()>>,
    adapters: AdapterOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AdapterOptions {
    matrix: Option<AdapterConfig<MatrixConfig, String>>,
    #[serde(alias = "pager_duty")]
    pagerduty: Option<AdapterConfig<PagerDutyConfig, PagerDutyLevel>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PagerDutyLevel {
    integration_key: String,
    payload_severity: PayloadSeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AdapterConfig<T, L> {
    enabled: bool,
    escation: EscalationConfig<L>,
    config: Option<T>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EscalationConfig<T> {
    window: u64,
    levels: Vec<T>,
}

#[derive(StructOpt, Debug)]
#[structopt(name = "matrixbot")]
struct Cli {
    #[structopt(short, long)]
    config: String,
}

pub async fn run() -> Result<()> {
    let cli = Cli::from_args();

    // Initial setup and config.
    info!("Logger initialized");
    info!(
        "Opening config at {}",
        std::fs::canonicalize(&cli.config)?
            .to_str()
            .ok_or_else(|| anyhow!("Path to config is not valid unicode"))?
    );

    let content = std::fs::read_to_string(&cli.config)?;
    let config: Config = serde_yaml::from_str(&content)?;

    info!("Preparing adapter config data");

    info!("Setting up database {:?}", config.database);
    let db = database::Database::new(config.database).await?;

    // Starting adapters.
    let span = info_span!("starting_adapter_clients");
    span.in_scope(|| {
        info!("Starting clients and background tasks");
    });

    // Start adapters with their appropriate tasks.
    let adapters = config.adapters;
    if let Some(_matrix) = adapters.matrix {
        //start_matrix_tasks(matrix, db.clone(), &role_index).await?;
    }

    if let Some(_pagerduty) = adapters.pagerduty {
        //start_pager_duty_tasks(pagerduty, db.clone(), &role_index).await?;
    }

    // Starting webhook.
    info!("Starting API server");
    webhook::run_api_server(&config.listener, db).await?;

    loop {
        sleep(Duration::from_secs(u64::MAX)).await;
    }
}
