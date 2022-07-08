use crate::adapter::{Adapter, MatrixClient};
use crate::database::Database;
use crate::primitives::{
    Acknowledgement, AlertContext, ChannelId, Escalation, IncrementedPendingAlerts,
    NotifyNewlyInserted, PendingAlerts, Role, UserConfirmation,
};
use crate::{Result, UserInfo};
use actix::prelude::*;
use actix_broker::{BrokerIssue, BrokerSubscribe};
use ruma::RoomId;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::mpsc::{unbounded_channel, Receiver, Sender};
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};

const INTERVAL: u64 = 10;

pub struct EscalationService {
    db: Database,
    window: Duration,
    permission: Arc<PermissionType>,
}

impl EscalationService {
    async fn run(self) {
        loop {
            match self.local().await {
                Ok(_) => {}
                Err(_) => {}
            }

            sleep(Duration::from_secs(INTERVAL)).await;
        }
    }
    async fn local(&self) -> Result<()> {
        let pending = self.db.get_pending(Some(self.window)).await?;
        for alert in pending.alerts {
            //let escalation = alert.into_escalation();
            //self.adapter_matrix.send(escalation)?;
            // TODO: Update database.
        }

        Ok(())
    }
}

pub struct PermissionHandler<T> {
    adapter_matrix: Sender<Escalation<RoomId>>,
    levels: Vec<T>,
    per_type: PermissionType,
    db: Database,
}

pub enum PermissionType {
    Users(HashSet<UserInfo>),
    MinRole {
        min: Role,
        roles: Vec<(Role, Vec<UserInfo>)>,
    },
    Roles(Vec<(Role, Vec<UserInfo>)>),
    // TODO: Rename
    EscalationLevel,
}

impl<T> PermissionHandler<T> {
    async fn handle_ack(&self, ack: Acknowledgement) -> Result<UserConfirmation> {
        let res = match &self.per_type {
            PermissionType::Users(users) => {
                if users.iter().any(|info| info.matches(&ack.user)) {
                    // Acknowledge alert.
                    if self.db.acknowledge_alert(&ack.alert_id, &ack.user).await? {
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
                    if self.db.acknowledge_alert(&ack.alert_id, &ack.user).await? {
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
                    if self.db.acknowledge_alert(&ack.alert_id, &ack.user).await? {
                        UserConfirmation::AlertAcknowledged(ack.alert_id)
                    } else {
                        UserConfirmation::AlertNotFound
                    }
                } else {
                    UserConfirmation::NoPermission
                }
            }
            PermissionType::EscalationLevel => {
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
    }
}
