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
use actix::prelude::*;
use adapter::matrix::{MatrixClient, MatrixConfig};
use adapter::pagerduty::{PagerDutyClient, PagerDutyConfig, PayloadSeverity};
use database::{Database, DatabaseConfig};
use escalation::{EscalationService, PermissionType};
use primitives::{ChannelId, Role, User};
use ruma::RoomId;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::time::Duration;
use structopt::StructOpt;

mod adapter;
mod database;
mod escalation;
mod primitives;
#[cfg(test)]
mod tests;
mod user_request;
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
    users: Vec<UserInfo>,
    roles: Vec<RoleInfo>,
}

// TODO: Move to primitives.
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct UserInfo {
    name: String,
    email: Option<String>,
    matrix: Option<String>,
    pagerduty: Option<String>,
    #[cfg(test)]
    mocker: Option<String>,
}

impl UserInfo {
    fn matches(&self, user: &User) -> bool {
        match user {
            User::Matrix(name) => self.matrix.as_ref().map(|s| s == name).unwrap_or(false),
            #[cfg(test)]
            User::Mocker(name) => self.mocker.as_ref().map(|s| s == name).unwrap_or(false),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RoleInfo {
    name: Role,
    members: Vec<String>,
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
    acks: AckType,
    levels: Vec<T>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum AckType {
    Users(Vec<String>),
    MinRole(Role),
    Roles(Vec<Role>),
    EscalationLevel,
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

    info!("Creating role index");
    let role_index = RoleIndex::create_index(config.users, config.roles)?;

    info!("Setting up database {:?}", config.database);
    let db = database::Database::new(config.database).await?;

    // Starting adapters.
    let span = info_span!("starting_adapter_clients");
    span.in_scope(|| {
        info!("Starting clients and background tasks");
    });

    // Start adapters with their appropriate tasks.
    let adapters = config.adapters;
    if let Some(matrix) = adapters.matrix {
        start_matrix_tasks(matrix, db.clone(), &role_index).await?;
    }

    if let Some(pagerduty) = adapters.pagerduty {
        start_pager_duty_tasks(pagerduty, db.clone(), &role_index).await?;
    }

    // Starting webhook.
    info!("Starting API server");
    webhook::run_api_server(&config.listener, db).await?;

    loop {
        sleep(Duration::from_secs(u64::MAX)).await;
    }
}

// Convenience function for processing the matrix configuration and starting all
// necessary tasks.
async fn start_matrix_tasks(
    adapter: AdapterConfig<MatrixConfig, String>,
    db: Database,
    role_index: &RoleIndex,
) -> Result<()> {
    let _levels = adapter
        .escation
        .levels
        .iter()
        .map(|level| {
            RoomId::from_str(level)
                .map(ChannelId::Matrix)
                .map_err(|err| err.into())
        })
        .collect::<Result<Vec<ChannelId>>>()?;

    /*
    start_tasks(
        db,
        MatrixClient::new(
            adapter
                .config
                .ok_or_else(|| anyhow!("no matrix configuration provided"))?,
        )
        .await?
        .start(),
        adapter.escation,
        role_index,
        levels,
    )
     */
    unimplemented!()
}

// Convenience function for processing the PagerDuty configuration and starting all
// necessary tasks.
async fn start_pager_duty_tasks(
    adapter: AdapterConfig<PagerDutyConfig, PagerDutyLevel>,
    db: Database,
    role_index: &RoleIndex,
) -> Result<()> {
    // TODO: Consider struct destructuring to avoid clones.

    /*
    let _levels = adapter
        .escation
        .levels
        .iter()
        .map(|level| ChannelId::PagerDuty {
            integration_key: level.integration_key.clone(),
            payload_severity: level.payload_severity,
        })
        .collect();

    start_tasks(
        db,
        PagerDutyClient::new(
            adapter
                .config
                .ok_or_else(|| anyhow!("no PagerDuty configuration provided"))?,
        )
        .start(),
        adapter.escation,
        role_index,
        levels,
    )
     */

    unimplemented!()
}

struct RoleIndex {
    users: HashMap<String, UserInfo>,
    roles: Vec<(Role, Vec<UserInfo>)>,
}

impl RoleIndex {
    fn create_index(users: Vec<UserInfo>, roles: Vec<RoleInfo>) -> Result<Self> {
        // Create a lookup table for all user entries, searchable by name.
        let mut lookup = HashMap::new();
        for user in users {
            lookup.insert(user.name.clone(), user);
        }

        // Create a role index by grouping users based on roles. Users can appear in
        // multiple roles or in none.
        let mut index = vec![];
        for role in roles {
            let mut user_infos = vec![];
            for member in role.members {
                let info = lookup.get(&member).ok_or_else(|| {
                    anyhow!(
                        "user {} specified in role {} does not exit",
                        member,
                        role.name
                    )
                })?;
                user_infos.push(info.clone());
            }
            index.push((role.name, user_infos));
        }

        Ok(RoleIndex {
            users: lookup,
            roles: index,
        })
    }
    /// Fetches the necessary data from the role index and converts it into a
    /// `PermissionType`, which is used by the escalation service to check for
    /// permissions when receiving acknowledgments.
    fn as_permission_type(&self, ack_type: AckType) -> Result<PermissionType> {
        let ty = match ack_type {
            AckType::Users(users) => {
                let mut infos = HashSet::new();
                for raw in &users {
                    let info = self
                        .users
                        .get(raw)
                        .ok_or_else(|| anyhow!("user {} is not configured", raw))?;
                    infos.insert(info.clone());
                }

                PermissionType::Users(infos)
            }
            AckType::MinRole(role) => PermissionType::MinRole {
                min: role,
                roles: self.roles.clone(),
            },
            AckType::Roles(roles) => PermissionType::Roles(
                self.roles
                    .iter()
                    .cloned()
                    .filter(|(role, _)| roles.contains(role))
                    .collect(),
            ),
            AckType::EscalationLevel => PermissionType::EscalationLevel,
        };

        Ok(ty)
    }
}
