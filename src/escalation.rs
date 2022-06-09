use crate::adapter::{MatrixClient, PagerDutyClient};
use crate::database::Database;
use crate::primitives::{Acknowledgement, ChannelId, NotifyAlert, Role, User, UserConfirmation};
use crate::{AckType, Result, RoleInfo, UserInfo};
use actix::prelude::*;
use actix_broker::BrokerSubscribe;
use matrix_sdk::instant::SystemTime;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

const INTERVAL: u64 = 10;

pub enum PermissionType {
    Users(HashSet<UserInfo>),
    MinRole {
        min: Role,
        roles: Vec<(Role, Vec<UserInfo>)>,
    },
    Roles(Vec<(Role, Vec<UserInfo>)>),
    EscalationLevel(Option<ChannelId>),
}

/*
impl Permissions {
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
*/

pub struct EscalationService<T: Actor> {
    db: Database,
    window: Duration,
    adapter: Addr<T>,
    is_locked: Arc<RwLock<bool>>,
    permission: Arc<PermissionType>,
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
                // TODO!!: Increment levels.
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

        let acks = Arc::clone(&self.permission);

        let f = async move {
            let res = match &*acks {
                PermissionType::Users(users) => {
                    if users.iter().any(|info| info.matches(&ack.user)) {
                        // Acknowledge alert.
                        db.ack(&ack.alert_id, &ack.user).await?;
                        UserConfirmation::AlertAcknowledged(ack.alert_id)
                    } else {
                        UserConfirmation::NoPermission
                    }
                }
                PermissionType::MinRole { min, roles } => {
                    let min_idx = roles.iter().position(|(role, _)| role == min).unwrap();

                    if roles
                        .iter()
                        .enumerate()
                        .filter(|(_, (_, users))| users.iter().any(|info| info.matches(&ack.user)))
                        .find(|(idx, _)| idx >= &min_idx)
                        .is_some()
                    {
                        // Acknowledge alert.
                        db.ack(&ack.alert_id, &ack.user).await?;
                        UserConfirmation::AlertAcknowledged(ack.alert_id)
                    } else {
                        UserConfirmation::NoPermission
                    }
                }
                PermissionType::Roles(roles) => {
                    if roles
                        .iter()
                        .find(|(_, users)| users.iter().any(|info| info.matches(&ack.user)))
                        .is_some()
                    {
                        // Acknowledge alert.
                        db.ack(&ack.alert_id, &ack.user).await?;
                        UserConfirmation::AlertAcknowledged(ack.alert_id)
                    } else {
                        UserConfirmation::NoPermission
                    }
                }
                PermissionType::EscalationLevel(level) => {
                    unimplemented!()
                    /*
                    if false {
                        // Acknowledge alert.
                        db.ack(&ack.alert_id, &ack.user).await?;
                        UserConfirmation::AlertAcknowledged(ack.alert_id)
                    } else {
                        UserConfirmation::AlertOutOfScope
                    }
                     */
                }
            };

            Ok(res)
        };

        Box::pin(f.into_actor(self))
    }
}
