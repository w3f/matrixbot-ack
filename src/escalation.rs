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
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};

const INTERVAL: u64 = 10;

pub struct EscalationService {
    db: Database,
    window: Duration,
    adaps: Vec<Arc<Box<dyn Adapter>>>,
}

struct AdapterContext<T: Adapter> {
    db: Database,
    adapter: UnboundedSender<Notification>,
    levels: Vec<T>,
    permission: PermissionType,
    // This turns into `None` after `run_request_handler` is therefore
    // not reused anymore.
    req_hander: Option<UnboundedReceiver<UserAction>>,
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
    async fn register_adapter<T: Adapter>(mut self, adapter: T) -> Self {
        self.adaps.push(Arc::new(Box::new(adapter)));
        self
    }
    async fn run(mut self) {
        loop {
            // TODO: Handle unwrap
            let pending = self.db.get_pending(Some(self.window)).await.unwrap();
            for alert in pending.alerts {
                // Don't exit on error if an adapter failed, continue with the rest.
                let dbg_id = alert.id;

                // TODO: Notify
                for adapter in &self.adaps {
                    match adapter.notify().await {
                        Ok(_) => {}
                        Err(_) => {}
                    }
                }

                // TODO: Update database with new idx, track which adapters failed
            }

            sleep(Duration::from_secs(INTERVAL)).await;
        }
    }
    async fn run_request_handler(&self) {
        for adapter in &self.adaps {
            let adapter = Arc::clone(&adapter);

            let db = self.db.clone();
            tokio::spawn(async move {
                while let Some(action) = adapter.endpoint_request().await {
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
    }
}

/*
impl<T> AdapterContext<T>
where
    T: 'static + Send + Sync + std::fmt::Debug + Clone,
{
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
*/
