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

use actix::clock::sleep;
use actix::{prelude::*, SystemRegistry};
use adapter::matrix::{MatrixClient, MatrixConfig};
use adapter::pagerduty::{PagerDutyClient, PagerDutyConfig};
use database::DatabaseConfig;
use std::time::Duration;
use structopt::StructOpt;
use tracing::Instrument;

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
    database: DatabaseConfig,
    listener: String,
    escalation: Option<EscalationConfig>,
    adapters: AdapterOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AdapterOptions {
    matrix: Option<AdapterConfig<MatrixConfig>>,
    #[serde(alias = "pager_duty")]
    pagerduty: Option<AdapterConfig<PagerDutyConfig>>,
}

impl AdapterOptions {
    fn into_mappings(
        self,
        root_escalation: Option<EscalationConfig>,
    ) -> Result<Vec<AdapterMapping>> {
        let mut mappings = vec![];

        if let Some(matrix) = self.matrix {
            if matrix.enabled {
                mappings.push(AdapterMapping::Matrix {
                    client_config: matrix
                        .config
                        .ok_or_else(|| anyhow!("Matrix config not provided"))?,
                    escalation_config: matrix
                        .escation
                        .or(root_escalation.clone())
                        .unwrap_or(EscalationConfig::disabled()),
                });
            }
        }

        if let Some(pagerduty) = self.pagerduty {
            if pagerduty.enabled {
                mappings.push(AdapterMapping::PagerDuty {
                    client_config: pagerduty
                        .config
                        .ok_or_else(|| anyhow!("PagerDuty config not provided"))?,
                    escalation_config: pagerduty
                        .escation
                        .or(root_escalation.clone())
                        .unwrap_or(EscalationConfig::disabled()),
                });
            }
        }

        Ok(mappings)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AdapterConfig<T> {
    enabled: bool,
    escation: Option<EscalationConfig>,
    config: Option<T>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OnOff<T> {
    enabled: bool,
    config: Option<T>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EscalationConfig {
    enabled: bool,
    escalation_window: Option<u64>,
}

impl EscalationConfig {
    fn disabled() -> Self {
        EscalationConfig {
            enabled: false,
            escalation_window: None,
        }
    }
}

#[derive(StructOpt, Debug)]
#[structopt(name = "matrixbot")]
struct Cli {
    #[structopt(short, long)]
    config: String,
}

use primitives::NotifyAlert;

enum AdapterMapping {
    Matrix {
        client_config: MatrixConfig,
        escalation_config: EscalationConfig,
    },
    PagerDuty {
        client_config: PagerDutyConfig,
        escalation_config: EscalationConfig,
    },
}

async fn start_clients(adapters: Vec<AdapterMapping>) -> Result<()> {
    fn start_tasks<T>(client: T, escalation_config: EscalationConfig)
    where
        T: Actor + Handler<NotifyAlert>,
        <T as Actor>::Context: actix::dev::ToEnvelope<T, NotifyAlert>,
    {
        escalation::EscalationService::<T>::new().start();
        user_request::RequestHandler::<T>::new().start();
    }

    for adapter in adapters {
        match adapter {
            AdapterMapping::Matrix {
                client_config,
                escalation_config,
            } => {
                start_tasks(MatrixClient::new(client_config).await?, escalation_config);
            }
            AdapterMapping::PagerDuty {
                client_config,
                escalation_config,
            } => {
                start_tasks(PagerDutyClient::new(client_config), escalation_config);
            }
        }
    }

    Ok(())
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
    let mappings = config.adapters.into_mappings(config.escalation)?;

    info!("Setting up database {:?}", config.database);
    let db = database::Database::new(config.database).await?;

    // Starting adapters.
    let span = info_span!("starting_adapter_clients");
    span.in_scope(|| {
        info!("Starting clients and background tasks");
    });

    start_clients(mappings).instrument(span).await?;

    // Starting webhook.
    info!("Starting API server");
    webhook::run_api_server(&config.listener, db).await?;

    loop {
        sleep(Duration::from_secs(u64::MAX)).await;
    }
}
