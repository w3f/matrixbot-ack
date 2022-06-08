#[macro_use]
extern crate tracing;
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
    database: database::DatabaseConfig,
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

use primitives::NotifyAlert;

enum AdapterConfig {
    Matrix(()),
    PagerDuty(()),
}

async fn start_clients(adapters: Vec<AdapterConfig>) -> Result<()> {
    fn start_tasks<T>(client: T)
    where
        T: Actor + Handler<NotifyAlert>,
        <T as Actor>::Context: actix::dev::ToEnvelope<T, NotifyAlert>,
    {
        escalation::EscalationService::<T>::new().start();
        user_request::RequestHandler::<T>::new().start();
    }

    for adapter in adapters {
        match adapter {
            AdapterConfig::Matrix(config) => {
                start_tasks(MatrixClient::new_tmp(config).await?);
            }
            AdapterConfig::PagerDuty(config) => {
                start_tasks(PagerDutyClient::new(config));
            }
        }
    }

    Ok(())
}

pub async fn run() -> Result<()> {
    let cli = Cli::from_args();

    info!("Logger initialized");

    info!(
        "Opening config at {}",
        std::fs::canonicalize(&cli.config)?
            .to_str()
            .ok_or_else(|| anyhow!("Path to config is not valid unicode"))?
    );

    let content = std::fs::read_to_string(&cli.config)?;
    let config: Config = serde_yaml::from_str(&content)?;

    info!("Setting up database {:?}", config.database);
    let db = database::Database::new(config.database).await?;

    info!("Starting API server");
    webhook::run_api_server(&config.listener, db).await?;

    loop {
        sleep(Duration::from_secs(u64::MAX)).await;
    }
}
