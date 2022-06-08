use crate::adapter::{MatrixClient, PagerDutyClient};
use crate::database::Database;
use crate::primitives::{Acknowledgement, ChannelId, NotifyAlert, Role, User, UserConfirmation};
use crate::Result;
use actix::prelude::*;
use actix_broker::BrokerSubscribe;
use matrix_sdk::instant::SystemTime;
use std::collections::HashMap;
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
    roles: Vec<(Role, Vec<User>)>,
}

impl RoleIndex {
    pub fn user_is_permitted(&self, user: &User, expected: &[Role]) -> bool {
        self.roles
            .iter()
            .filter(|(_, users)| users.contains(user))
            .any(|(role, _)| expected.contains(role))
    }
    pub fn is_above_minimum(&self, min: &Role, user: &User) -> bool {
        let min_idx = self.roles.iter().position(|(role, _)| role == min).unwrap();

        self.roles
            .iter()
            .enumerate()
            .filter(|(_, (_, users))| users.contains(&user))
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
    pub fn new() -> Self {
        unimplemented!()
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
                    if roles.user_is_permitted(&ack.user, &allowed) {
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
