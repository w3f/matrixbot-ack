use crate::adapter::{Adapter, MatrixClient};
use crate::database::Database;
use crate::primitives::{
    Acknowledgement, AlertContext, ChannelId, Escalation, IncrementedPendingAlerts,
    NotifyNewlyInserted, PendingAlerts, Role, UserConfirmation,
};
use crate::{Result, UserInfo};
use actix::prelude::*;
use actix_broker::{BrokerSubscribe, BrokerIssue};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
use tokio::sync::broadcast::{channel, Sender, Receiver};

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

pub struct EscalationService {
    db: Database,
    window: Duration,
    permission: Arc<PermissionType>,
    broadcaster: Sender<Escalation>,
}

impl EscalationService {
    async fn run(self) {
        loop {
            match self.local().await {
                Ok(_) => {},
                Err(_) => {},
            }

            sleep(Duration::from_secs(INTERVAL)).await;
        }
    }
    async fn local(&self) -> Result<()> {
        let pending = self.db.get_pending(Some(self.window)).await?;
        for alert in pending.alerts {
            let escalation = alert.into_escalation();
            self.broadcaster.send(escalation)?;
            // TODO: Update database.
        }

        Ok(())
    }
}

/*
impl Handler<Acknowledgement> for EscalationService {
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
*/
