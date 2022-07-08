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

impl EscalationService {
    async fn register_adapter<T: Adapter>(mut self, adapter: T) -> Self {
        self.adaps.push(Arc::new(Box::new(adapter)));
        self
    }
    async fn run(self) {
        self.run_request_handler();

        loop {
            for adapter in &self.adaps {
                // TODO: Handle unwrap
                let pending = self
                    .db
                    .get_pending(Some(self.window), Some(adapter.name()))
                    .await
                    .unwrap();

                // Notify adapter about escalation
                for alert in &pending.alerts {
                    match adapter
                        .notify(Notification::Escalation {
                            id: alert.id,
                            alert: alert.alert.clone(),
                            current_room_idx: alert.level_idx,
                        })
                        .await
                    {
                        Ok(_) => {
                            info!(
                                "Notified {} adapter about escalation ID {}",
                                adapter.name(),
                                alert.id
                            );

                            // TODO: Handle unwrap
                            self.db
                                .mark_delivered(alert.id, adapter.name())
                                .await
                                .unwrap();
                        }
                        Err(err) => error!(
                            "failed to notify {} adapter about escalation ID {}: {:?}",
                            adapter.name(),
                            alert.id,
                            err
                        ),
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
                        Command::Pending => match db.get_pending(None, None).await {
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
                                // TODO: Check response
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
