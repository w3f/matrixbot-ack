use crate::adapter::{Adapter, MatrixClient};
use crate::database::Database;
use crate::primitives::{
    Acknowledgement, AlertContext, ChannelId, Command, Escalation, IncrementedPendingAlerts,
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
    permission: Arc<PermissionType>,
    adapter_matrix: AdapterContext<RoomId>,
}

struct AdapterContext<T> {
    adapter: Sender<Escalation<T>>,
    levels: Vec<T>,
    per_type: PermissionType,
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
        // Start Matrix request handler
        let mut rh = std::mem::take(&mut self.adapter_matrix.req_hander);
        let db = self.db.clone();
        tokio::spawn(async move {
            let rh = rh.as_mut().unwrap();
            Self::run_request_handler(db, rh).await;
        });

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
            match self.adapter_matrix.notify_escalation(alert).await {
                Ok(_) => {}
                Err(_) => {}
            }
        }

        Ok(())
    }
    async fn run_request_handler<T>(db: Database, req_handler: &mut Receiver<UserAction<T>>) {
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
        }
    }
}

impl<T> AdapterContext<T>
where
    T: 'static + Send + Sync + std::fmt::Debug + Clone,
{
    async fn notify_escalation(&self, alert: AlertContext) -> Result<()> {
        let (escalation, _new_level_idx) = alert.into_escalation(&self.levels);
        self.adapter.send(escalation).await?;

        // TODO: Update database with new idx

        Ok(())
    }
    async fn handle_ack(&self, db: &Database, ack: Acknowledgement) -> Result<UserConfirmation> {
        let res = match &self.per_type {
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
