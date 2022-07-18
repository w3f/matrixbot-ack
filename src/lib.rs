#[macro_use]
extern crate tracing;
#[macro_use]
extern crate anyhow;
#[macro_use]
extern crate serde;
#[macro_use]
extern crate async_trait;

use adapter::email::{EmailConfig, EmailLevel};
use adapter::matrix::MatrixConfig;
use adapter::pagerduty::{PagerDutyConfig, PagerDutyLevel};
use database::DatabaseConfig;
use structopt::StructOpt;
use tokio::time::Duration;

use crate::adapter::email::EmailClient;
use crate::adapter::{MatrixClient, PagerDutyClient};
use crate::escalation::EscalationService;

mod adapter;
mod database;
mod escalation;
mod primitives;
#[cfg(test)]
mod tests;
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
    escalation: EscalationConfig,
    adapter: AdapterOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AdapterOptions {
    matrix: Option<AdapterConfig<MatrixConfig, String>>,
    #[serde(alias = "pager_duty")]
    pagerduty: Option<AdapterConfig<PagerDutyConfig, PagerDutyLevel>>,
    email: Option<AdapterConfig<EmailConfig, EmailLevel>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AdapterConfig<T, L> {
    enabled: bool,
    config: Option<T>,
    levels: Option<Vec<L>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EscalationConfig {
    window: u64,
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

    // Starting adapters.
    info!("Starting clients and background tasks");

    // Setup escalation service.
    let mut escalation =
        EscalationService::new(db.clone(), Duration::from_secs(config.escalation.window));

    // Start adapters with their appropriate tasks.
    let adapters = config.adapter;
    if let Some(matrix_conf) = adapters.matrix {
        if matrix_conf.enabled {
            info!("Matrix adapter is enabled, starting...");

            let matrix = MatrixClient::new(
                matrix_conf
                    .config
                    .ok_or_else(|| anyhow!("Matrix config is missing"))?,
                matrix_conf
                    .levels
                    .ok_or_else(|| anyhow!("Matrix levels are not configured"))?,
            )
            .await?;
            escalation.register_adapter(matrix);

            info!("Matrix adapter setup completed");
        }
    }

    if let Some(pagerduty_conf) = adapters.pagerduty {
        if pagerduty_conf.enabled {
            info!("PagerDuty adapter is enabled, starting...");

            let pagerduty = PagerDutyClient::new(
                pagerduty_conf
                    .config
                    .ok_or_else(|| anyhow!("PagerDuty config is missing"))?,
                pagerduty_conf
                    .levels
                    .ok_or_else(|| anyhow!("PagerDuty levels are not configured"))?,
            )
            .await;

            escalation.register_adapter(pagerduty);
            info!("PagerDuty adapter setup completed");
        }
    }

    if let Some(email_conf) = adapters.email {
        if email_conf.enabled {
            info!("Email adapter is enabled, starting...");

            let email = EmailClient::new(
                email_conf
                    .config
                    .ok_or_else(|| anyhow!("Email config is missing"))?,
                email_conf
                    .levels
                    .ok_or_else(|| anyhow!("Email levels are not configured"))?,
            )
            .await?;

            escalation.register_adapter(email);
            info!("Email adapter setup completed");
        }
    }

    info!("Starting escalation background service");
    escalation.run_service().await;

    // Starting webhook.
    info!("Starting API server on endpoint {}", config.listener);
    let server = webhook::run_api_server(&config.listener, db).await?;

    info!("Application setup completed! Listening...");
    server.await.map_err(|err| err.into())
}
