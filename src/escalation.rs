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
    async fn run(self) {
        self.run_request_handler();

        loop {
            // TODO: Handle unwrap
            let pending = self.db.get_pending(Some(self.window)).await.unwrap();

            // TODO: Repeated attempts.
            for alert in pending.alerts {
                // Notify adapter
                for adapter in &self.adaps {
                    match adapter
                        .notify(Notification::Escalation {
                            id: alert.id,
                            alert: alert.alert.clone(),
                            current_room_idx: alert.level_idx,
                        })
                        .await
                    {
                        Ok(_) => {}
                        Err(_) => {}
                    }
                }

                // TODO: Update database with new idx, track which adapters failed
            }

            sleep(Duration::from_secs(INTERVAL)).await;
        }
    }
    fn run_request_handler(&self) {
        for adapter in &self.adaps {
            let adapter = Arc::clone(adapter);
            let db = self.db.clone();

            let others: Vec<Arc<Box<dyn Adapter>>> = self
                .adaps
                .iter()
                .filter(|other| other.name() != adapter.name())
                .map(|other| Arc::clone(other))
                .collect();

            tokio::spawn(async move {
                while let Some(action) = adapter.endpoint_request().await {
                    let message = match action.command {
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

                    // If an alert was acknowledged, notify the other adapters about it.
                    match message {
                        UserConfirmation::AlertAcknowledged(alert_id) => {
                            for other in &others {
                                let x = other
                                    .notify(Notification::Acknowledged {
                                        id: alert_id,
                                        acked_by: action.user.clone(),
                                    })
                                    .await;
                            }
                        }
                        _ => {}
                    }

                    adapter.respond(message).await;
                }
            });
        }
    }
}
