use crate::adapter::Adapter;
use crate::database::Database;
use crate::primitives::{
    Acknowledgement, AlertDelivery, ChannelId, Escalation, IncrementedPendingAlerts,
    NotifyNewlyInserted, PendingAlerts, Role, UserConfirmation, AlertContext
};
use crate::{Result, UserInfo};
use actix::prelude::*;
use actix_broker::BrokerSubscribe;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

const INTERVAL: u64 = 10;

struct AdapterContext<T: Actor + Adapter> {
    adapter: Addr<T>,
    levels: Vec<<T as Adapter>::Channel>,
}

impl<T> AdapterContext<T>
where
    T: Adapter + Actor + Handler<Escalation<T>>,
    <T as Actor>::Context: actix::dev::ToEnvelope<T, Escalation<T>>,
{
    async fn escalate(&self, mut alert: AlertContext) -> (Escalation<T>, IncrementedPendingAlerts) {
        let alert_id = alert.id;
        let (escalation, new_level_idx) = alert.into_escalation::<T>(&self.levels);

        (
        escalation,
        IncrementedPendingAlerts {
            id: alert_id,
            new_level_idx
        }
        )
    }
}

pub enum PermissionType {
    Users(HashSet<UserInfo>),
    MinRole {
        min: Role,
        roles: Vec<(Role, Vec<UserInfo>)>,
    },
    Roles(Vec<(Role, Vec<UserInfo>)>),
    EscalationLevel(Option<ChannelId>),
}

pub struct EscalationService<T: Actor> {
    db: Database,
    window: Duration,
    adapter: Addr<T>,
    is_locked: Arc<RwLock<bool>>,
    permission: Arc<PermissionType>,
    levels: Arc<Vec<ChannelId>>,
}

impl<T: Actor> EscalationService<T> {
    pub fn new(
        db: Database,
        window: Duration,
        adapter: Addr<T>,
        permission: PermissionType,
        levels: Vec<ChannelId>,
    ) -> Self {
        EscalationService {
            db,
            window,
            adapter,
            is_locked: Arc::new(RwLock::new(false)),
            permission: Arc::new(permission),
            levels: Arc::new(levels),
        }
    }
}

impl<T> Actor for EscalationService<T>
where
    T: Actor + Handler<AlertDelivery>,
    <T as Actor>::Context: actix::dev::ToEnvelope<T, AlertDelivery>,
{
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.subscribe_system_async::<NotifyNewlyInserted>(ctx);

        ctx.run_interval(Duration::from_secs(INTERVAL), |actor, _ctx| {
            let db = actor.db.clone();
            let addr = actor.adapter.clone();

            let levels = Arc::clone(&actor.levels);
            let is_locked = Arc::clone(&actor.is_locked);
            let window = actor.window;

            actix::spawn(async move {
                // Lock escalation process, do not overlap.
                let mut l = is_locked.write().await;
                *l = true;
                std::mem::drop(l);

                // Retrieve pending alerts.
                let pending = match db.get_pending(Some(window)).await {
                    Ok(pending) => pending,
                    Err(err) => {
                        error!("failed to retrieve pending alerts: {:?}", err);

                        // Unlock escalation process, ready to be picked up on the next interval.
                        let mut l = is_locked.write().await;
                        *l = false;

                        return;
                    }
                };

                // Notify adapter about each alert.
                for alert in pending.alerts {
                    // Increment the escalation level, if necessary
                    /*
                    let (delivery, new_idx) = alert.into_delivery(&levels);
                    let id = delivery.id;

                    // If delivery fails, it will be attempted again on the next interval.
                    match addr.send(delivery).await {
                        Ok(resp) => match resp {
                            Ok(_) => {
                                let _ = db.increment_alert_state(id, new_idx).await.map_err(|err| {
                                    error!("failed to increment alert state of {}, error: {:?}", id, err)
                                });
                            }
                            Err(err) => {
                                error!("failed to notify users about new alert: {:?}", err);
                            }
                        },
                        Err(err) => {
                            error!(
                                "actor mailbox error when attempting to notify about new alert: {:?}",
                                err
                            );
                        }
                    }
                    */
                }

                // Unlock escalation process, ready to be picked up on the next interval.
                let mut l = is_locked.write().await;
                *l = false;
            });
        });
    }
}

impl<T: Actor> Handler<NotifyNewlyInserted> for EscalationService<T>
where
    T: Actor + Handler<AlertDelivery>,
    <T as Actor>::Context: actix::dev::ToEnvelope<T, AlertDelivery>,
{
    type Result = ResponseActFuture<Self, ()>;

    fn handle(&mut self, inserted: NotifyNewlyInserted, _ctx: &mut Self::Context) -> Self::Result {
        let levels = Arc::clone(&self.levels);
        let addr = self.adapter.clone();
        let db = self.db.clone();

        let f = async move {
            for alert in inserted.alerts {
                /*
                let (delivery, _) = alert.into_delivery(&levels);
                let id = delivery.id;

                // If delivery fails, it will be attempted again by the
                // background interval in `<Self as Actor>::started`.
                match addr.send(delivery).await {
                    Ok(resp) => match resp {
                        Ok(_) => {
                            let _ = db.mark_delivered(id).await.map_err(|err| {
                                error!("failed to mark alert {} as delivered: {:?}", id, err)
                            });
                        }
                        Err(err) => {
                            error!("failed to notify users about new alert: {:?}", err);
                        }
                    },
                    Err(err) => {
                        error!(
                            "actor mailbox error when attempting to notify about new alert: {:?}",
                            err
                        );
                    }
                }
                */
            }
        };

        Box::pin(f.into_actor(self))
    }
}

impl<T: Actor> Handler<Acknowledgement> for EscalationService<T>
where
    T: Actor + Handler<AlertDelivery>,
    <T as Actor>::Context: actix::dev::ToEnvelope<T, AlertDelivery>,
{
    type Result = ResponseActFuture<Self, Result<UserConfirmation>>;

    fn handle(&mut self, ack: Acknowledgement, _ctx: &mut Self::Context) -> Self::Result {
        let db = self.db.clone();

        let acks = Arc::clone(&self.permission);

        let f = async move {
            let res = match &*acks {
                PermissionType::Users(users) => {
                    if users.iter().any(|info| info.matches(&ack.user)) {
                        // Acknowledge alert.
                        if db.acknowledge_alert(&ack.alert_id, &ack.user).await? {
                            UserConfirmation::AlertAcknowledged(ack.alert_id)
                        } else {
                            UserConfirmation::AlertNotFound
                        }
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
                        .any(|(idx, _)| idx >= min_idx)
                    {
                        // Acknowledge alert.
                        if db.acknowledge_alert(&ack.alert_id, &ack.user).await? {
                            UserConfirmation::AlertAcknowledged(ack.alert_id)
                        } else {
                            UserConfirmation::AlertNotFound
                        }
                    } else {
                        UserConfirmation::NoPermission
                    }
                }
                PermissionType::Roles(roles) => {
                    if roles
                        .iter()
                        .any(|(_, users)| users.iter().any(|info| info.matches(&ack.user)))
                    {
                        // Acknowledge alert.
                        if db.acknowledge_alert(&ack.alert_id, &ack.user).await? {
                            UserConfirmation::AlertAcknowledged(ack.alert_id)
                        } else {
                            UserConfirmation::AlertNotFound
                        }
                    } else {
                        UserConfirmation::NoPermission
                    }
                }
                PermissionType::EscalationLevel(_level) => {
                    unimplemented!()
                    /*
                    if false {
                        // Acknowledge alert.
                        db.acknowledge_alert(&ack.alert_id, &ack.user).await?;
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
