use crate::adapter::{Adapter, MatrixClient};
use crate::database::Database;
use crate::primitives::{
    Acknowledgement, AlertContext, Command, IncrementedPendingAlerts, Notification,
    NotifyNewlyInserted, PendingAlerts, Role, UserAction, UserConfirmation,
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
    adapter_matrix: AdapterContext<RoomId>,
}

struct AdapterContext<T> {
    db: Database,
    adapter: Sender<Notification<T>>,
    levels: Vec<T>,
    permission: PermissionType,
    req_hander: Option<Receiver<UserAction<T>>>,
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

impl EscalationService {
    async fn run(mut self) {
        // Start request handlers
        self.adapter_matrix.run_request_handler().await;

        loop {
            if let Err(err) = self.local().await {
                error!("escalation even loop: {:?}", err);
            }

            sleep(Duration::from_secs(INTERVAL)).await;
        }
    }
    async fn local(&self) -> Result<()> {
        let pending = self.db.get_pending(Some(self.window)).await?;
        for alert in pending.alerts {
            // Don't exit on error if an adapter failed, continue with the rest.
            let dbg_id = alert.id;

            match self.adapter_matrix.notify_escalation(alert).await {
                Ok(_) => {
                    info!("notified matrix adapter about alert: {:?}", dbg_id);
                }
                Err(err) => {
                    error!("failed to notify matrix adapter: {:?}", err);
                }
            }

            // TODO: Update database with new idx, track which adapters failed
        }

        Ok(())
    }
}

impl<T> AdapterContext<T>
where
    T: 'static + Send + Sync + std::fmt::Debug + Clone,
{
    async fn run_request_handler(&mut self) {
        let db = self.db.clone();

        let mut req_handler = std::mem::take(&mut self.req_hander).unwrap();

        tokio::spawn(async move {
            while let Some(action) = req_handler.recv().await {
                let res = match action.command {
                    Command::Ack(alert_id) => {
                        // TODO
                        unimplemented!()
                    }
                    Command::Pending => match db.get_pending(None).await {
                        Ok(pending) => UserConfirmation::PendingAlerts(pending),
                        Err(err) => {
                            error!("failed to retrieve pending alerts: {:?}", err);
                            UserConfirmation::InternalError
                        }
                    },
                    Command::Help => UserConfirmation::Help,
                };

                // TODO
            }
        });
    }
    async fn notify_escalation(&self, alert: AlertContext) -> Result<()> {
        let (escalation, _new_level_idx) = alert.into_escalation(&self.levels);
        self.adapter.send(escalation).await?;

        Ok(())
    }
    async fn handle_ack(&self, ack: Acknowledgement<T>) -> Result<UserConfirmation> {
        let res = match &self.permission {
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
