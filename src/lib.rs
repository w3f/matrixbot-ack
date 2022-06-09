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
use database::{Database, DatabaseConfig};
use escalation::PermissionType;
use primitives::{ChannelId, NotifyAlert, Role, User};
use std::collections::{HashMap, HashSet};
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
    escalation: Option<EscalationConfig<()>>,
    adapters: AdapterOptions,
    users: Vec<UserInfo>,
    roles: Vec<RoleInfo>,
}

// TODO: Move to primitives.
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
struct UserInfo {
    name: String,
    email: Option<String>,
    matrix: Option<String>,
    pagerduty: Option<String>,
}

impl UserInfo {
    fn matches(&self, user: &User) -> bool {
        match user {
            User::Matrix(name) => self.matrix.as_ref().map(|s| s == name).unwrap_or(false),
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
    // TODO
    pagerduty: Option<AdapterConfig<PagerDutyConfig, ()>>,
}

impl AdapterOptions {
    fn into_mappings(self) -> Result<Vec<AdapterMapping>> {
        let mut mappings = vec![];

        if let Some(matrix) = self.matrix {
            if matrix.enabled {
                mappings.push(AdapterMapping::Matrix {
                    client_config: matrix
                        .config
                        .ok_or_else(|| anyhow!("Matrix config not provided"))?,
                    escalation_config: matrix.escation.unwrap_or_else(EscalationConfig::disabled),
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
                        .unwrap_or_else(EscalationConfig::disabled),
                });
            }
        }

        Ok(mappings)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AdapterConfig<T, L> {
    enabled: bool,
    escation: Option<EscalationConfig<L>>,
    config: Option<T>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EscalationConfig<T> {
    enabled: bool,
    window: Option<u64>,
    acks: Option<AckType>,
    levels: Option<T>,
}

// TODO: Rename
#[derive(Debug, Clone, Serialize, Deserialize)]
enum AckType {
    Users(Vec<String>),
    MinRole(Role),
    Roles(Vec<Role>),
    EscalationLevel,
}

impl<T> EscalationConfig<T> {
    fn disabled() -> Self {
        EscalationConfig {
            enabled: false,
            window: None,
            acks: None,
            levels: None,
        }
    }
}

#[derive(StructOpt, Debug)]
#[structopt(name = "matrixbot")]
struct Cli {
    #[structopt(short, long)]
    config: String,
}

enum AdapterMapping {
    Matrix {
        client_config: MatrixConfig,
        escalation_config: EscalationConfig<String>,
    },
    PagerDuty {
        client_config: PagerDutyConfig,
        escalation_config: EscalationConfig<()>,
    },
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
    let mappings = config.adapters.into_mappings()?;

    info!("Creating role index");
    let role_index = RoleIndex::create_index(config.users, config.roles)?;

    info!("Setting up database {:?}", config.database);
    let db = database::Database::new(config.database).await?;

    // Starting adapters.
    let span = info_span!("starting_adapter_clients");
    span.in_scope(|| {
        info!("Starting clients and background tasks");
    });

    start_clients(db.clone(), mappings, &role_index)
        .instrument(span)
        .await?;

    // Starting webhook.
    info!("Starting API server");
    webhook::run_api_server(&config.listener, db).await?;

    loop {
        sleep(Duration::from_secs(u64::MAX)).await;
    }
}

async fn start_clients(
    db: Database,
    adapters: Vec<AdapterMapping>,
    role_index: &RoleIndex,
) -> Result<()> {
    fn start_tasks<T, L>(
        db: Database,
        client: Addr<T>,
        escalation_config: EscalationConfig<L>,
        role_index: &RoleIndex,
    ) -> Result<()>
    where
        T: Actor + Handler<NotifyAlert>,
        <T as Actor>::Context: actix::dev::ToEnvelope<T, NotifyAlert>,
    {
        if escalation_config.enabled {
            let window = Duration::from_secs(
                escalation_config
                    .window
                    .ok_or_else(|| anyhow!("escalation window not defined"))?,
            );
            let permissions = role_index
                .as_permission_type(escalation_config.acks.ok_or_else(|| anyhow!("no acknowledgement type set"))?)?;

            escalation::EscalationService::<T>::new(db, window, client, permissions).start();
        }
        user_request::RequestHandler::<T>::new().start();

        Ok(())
    }

    for adapter in adapters {
        match adapter {
            AdapterMapping::Matrix {
                client_config,
                escalation_config,
            } => {
                start_tasks(
                    db.clone(),
                    MatrixClient::new(client_config).await?.start(),
                    escalation_config,
                    role_index,
                )?;
            }
            AdapterMapping::PagerDuty {
                client_config,
                escalation_config,
            } => {
                start_tasks(
                    db.clone(),
                    PagerDutyClient::new(client_config).start(),
                    escalation_config,
                    role_index,
                )?;
            }
        }
    }

    Ok(())
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
            AckType::EscalationLevel => PermissionType::EscalationLevel(None),
        };

        Ok(ty)
    }
}
