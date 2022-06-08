use crate::adapter::{MatrixClient, PagerDutyClient};
use crate::database::Database;
use crate::primitives::{Acknowledgement, ChannelId, NotifyAlert, Role, User, UserConfirmation};
use crate::{Result, UserInfo, RoleInfo};
use actix::prelude::*;
use actix_broker::BrokerSubscribe;
use matrix_sdk::instant::SystemTime;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

const INTERVAL: u64 = 10;

enum AckPermission {
    Users(Vec<User>),
    MinRole(Role),
    Roles(Vec<Role>),
    EscalationLevel(ChannelId),
}

pub struct RoleIndex {
    users: HashSet<UserInfo>,
    roles: Vec<(Role, Vec<UserInfo>)>,
}

impl RoleIndex {
    pub fn create_index(users: Vec<UserInfo>, roles: Vec<RoleInfo>) -> Result<Self> {
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
            users: lookup.into_iter().map(|(_, info)| info).collect(),
            roles: index,
        })
    }
    pub fn user_is_permitted(&self, user: &User) -> bool {
        self.users.iter().any(|info| info.matches(user))
    }
    pub fn user_is_in_role(&self, user: &User, expected: &[Role]) -> bool {
        self.roles
            .iter()
            .filter(|(_, users)| users.iter().any(|info| info.matches(&user)))
            .any(|(role, _)| expected.contains(role))
    }
    pub fn is_above_minimum(&self, min: &Role, user: &User) -> bool {
        let min_idx = self.roles.iter().position(|(role, _)| role == min).unwrap();

        self.roles
            .iter()
            .enumerate()
            .filter(|(_, (_, users))| users.iter().any(|info| info.matches(&user)))
            .find(|(idx, _)| idx >= &min_idx)
            .is_some()
    }
}

pub struct EscalationService<T: Actor> {
    db: Database,
    window: Duration,
    adapter: Addr<T>,
    is_locked: Arc<RwLock<bool>>,
    levels: Arc<LevelHandler>,
    acks: Arc<AckPermission>,
    roles: Arc<RoleIndex>,
}

impl<T: Actor> EscalationService<T> {
    pub fn new(db: Database, window: Duration, adapter: Addr<T>) -> Self {
        unimplemented!()
        /*
        EscalationService {
            db,
            window,
            adapter,
            is_locked: Arc::new(RwLock::new(false)),
            levels: Arc::new(),
        }
         */
    }
}

struct LevelHandler {
    levels: Vec<ChannelId>,
}

impl LevelHandler {
    pub fn is_above_level(&self, min: &ChannelId, check: &ChannelId) -> Option<bool> {
        let min_idx = self.levels.iter().position(|level| level == min)?;

        let is_above = self
            .levels
            .iter()
            .enumerate()
            .filter(|(_, level)| *level == check)
            .any(|(idx, _)| idx <= min_idx);

        Some(is_above)
    }
}

impl<T> Actor for EscalationService<T>
where
    T: Actor + Handler<NotifyAlert>,
    <T as Actor>::Context: actix::dev::ToEnvelope<T, NotifyAlert>,
{
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.run_interval(Duration::from_secs(INTERVAL), |actor, ctx| {
            let db = actor.db.clone();
            let addr = actor.adapter.clone();

            let is_locked = Arc::clone(&actor.is_locked);
            let window = actor.window;

            actix::spawn(async move {
                // Lock escalation process, do not overlap.
                let mut l = is_locked.write().await;
                *l = true;
                std::mem::drop(l);

                // TODO: Handle unwrap
                let mut pending = db.get_pending(Some(window)).await.unwrap();
                match addr.send(pending.clone()).await {
                    Ok(resp) => {
                        match resp {
                            Ok(_) => {
                                pending.update_timestamp_now();
                                let _ = db.update_pending(pending).await.map_err(|err| {
                                    // TODO: Log
                                });
                            }
                            Err(_err) => {
                                // TODO: Log
                            }
                        }
                    }
                    // TODO: Log
                    Err(_err) => {}
                }

                // Unlock escalation process, ready to be picked up on the next
                // window.
                let mut l = is_locked.write().await;
                *l = false;
            });
        });
    }
}

impl<T: Actor> Handler<Acknowledgement> for EscalationService<T>
where
    T: Actor + Handler<NotifyAlert>,
    <T as Actor>::Context: actix::dev::ToEnvelope<T, NotifyAlert>,
{
    type Result = ResponseActFuture<Self, Result<UserConfirmation>>;

    fn handle(&mut self, ack: Acknowledgement, ctx: &mut Self::Context) -> Self::Result {
        let db = self.db.clone();

        let roles = Arc::clone(&self.roles);
        let levels = Arc::clone(&self.levels);
        let acks = Arc::clone(&self.acks);

        let f = async move {
            let res = match &*acks {
                AckPermission::Users(users) => {
                    if users.contains(&ack.user) {
                        // Acknowledge alert.
                        db.ack(&ack.alert_id, &ack.user).await?;
                        UserConfirmation::AlertAcknowledged(ack.alert_id)
                    } else {
                        UserConfirmation::NoPermission
                    }
                }
                AckPermission::MinRole(min) => {
                    if roles.is_above_minimum(&min, &ack.user) {
                        // Acknowledge alert.
                        db.ack(&ack.alert_id, &ack.user).await?;
                        UserConfirmation::AlertAcknowledged(ack.alert_id)
                    } else {
                        UserConfirmation::NoPermission
                    }
                }
                AckPermission::Roles(allowed) => {
                    if roles.user_is_in_role(&ack.user, &allowed) {
                        // Acknowledge alert.
                        db.ack(&ack.alert_id, &ack.user).await?;
                        UserConfirmation::AlertAcknowledged(ack.alert_id)
                    } else {
                        UserConfirmation::NoPermission
                    }
                }
                AckPermission::EscalationLevel(level) => {
                    if levels
                        .is_above_level(&level, &level)
                        .ok_or_else(|| anyhow!("TODO"))?
                    {
                        // Acknowledge alert.
                        db.ack(&ack.alert_id, &ack.user).await?;
                        UserConfirmation::AlertAcknowledged(ack.alert_id)
                    } else {
                        UserConfirmation::AlertOutOfScope
                    }
                }
            };

            Ok(res)
        };

        Box::pin(f.into_actor(self))
    }
}
