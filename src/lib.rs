#[macro_use]
extern crate log;
#[macro_use]
extern crate anyhow;
#[macro_use]
extern crate serde;
#[macro_use]
extern crate async_trait;
#[macro_use]
extern crate actix;

use crate::adapter::matrix::{MatrixClient, MatrixConfig};
use crate::adapter::pagerduty::{PagerDutyClient, ServiceConfig};
use actix::clock::sleep;
use actix::{prelude::*, SystemRegistry};
use std::time::Duration;
use structopt::StructOpt;

mod adapter;
mod database;
mod escalation;
mod primitives;
mod user_request;
mod webhook;

pub type Result<T> = std::result::Result<T, anyhow::Error>;

const MIN_ESCALATION_WINDOW: u64 = 60; // 60 seconds

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
    listener: String,
    matrix: Option<OnOff<MatrixConfig>>,
    pager_duty: Option<OnOff<ServiceConfig>>,
    escalation: Option<OnOff<EscalationConfig>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OnOff<T> {
    enabled: bool,
    config: Option<T>,
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
    unimplemented!()

    /*
    env_logger::builder()
        .filter_module("system", log::LevelFilter::Debug)
        .init();

    info!("Logger initialized");

    info!(
        "Opening config at {}",
        std::fs::canonicalize(&cli.config)?
            .to_str()
            .ok_or_else(|| anyhow!("Path to config is not valid unicode"))?
    );

    let content = std::fs::read_to_string(&cli.config)?;
    let config: Config = serde_yaml::from_str(&content)?;

    // Retrieve relevant escalation data.
    let should_escalate = config
        .escalation
        .as_ref()
        .map(|c| c.enabled)
        .unwrap_or(false);

    let escalation_window = config
        .escalation
        .as_ref()
        .map(|c| c.config.as_ref().map(|c| c.escalation_window))
        .unwrap_or(Some(MIN_ESCALATION_WINDOW))
        .unwrap()
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
    let proc = processor::Processor::new(
        opt_db,
        escalation_window,
        should_escalate,
        config
            .pager_duty
            .as_ref()
            .map(|c| c.enabled)
            .unwrap_or(false),
    );
    SystemRegistry::set(proc.start());

    if let Some(matrix_ctx) = config.matrix {
        if matrix_ctx.enabled {
            let matrix_config = matrix_ctx
                .config
                .ok_or_else(|| anyhow!("matrix config not specified"))?;

            if matrix_config.rooms.is_empty() {
                return Err(anyhow!("No alert rooms have been configured"));
            }

            info!("Initializing Matrix client");
            // Only handle user commands if escalations are enabled.
            let matrix = MatrixClient::new(matrix_config, should_escalate).await?;
            SystemRegistry::set(matrix.start());
        }
    }

    info!("Initializing PagerDuty client");
    if let Some(pd_ctx) = config.pager_duty {
        if pd_ctx.enabled {
            let pd_config = pd_ctx.config.ok_or_else(|| anyhow!(""))?;

            let pager_duty = PagerDutyClient::new(pd_config);
            SystemRegistry::set(pager_duty.start());
        }
    }

    info!("Starting API server");
    webhook::run_api_server(&config.listener).await?;

    loop {
        sleep(Duration::from_secs(u64::MAX)).await;
    }
    */
}
