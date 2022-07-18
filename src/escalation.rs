use crate::adapter::Adapter;
use crate::database::{AcknowlegementResult, Database};
use crate::primitives::{Command, Notification, UserConfirmation};
use crate::Result;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

#[cfg(not(test))]
const INTERVAL: u64 = 5;
#[cfg(test)]
const INTERVAL: u64 = 1;

pub struct EscalationService {
    db: Database,
    window: Duration,
    adapters: Vec<Arc<Box<dyn Adapter>>>,
}

impl EscalationService {
    pub fn new(db: Database, window: Duration) -> Self {
        EscalationService {
            db,
            window,
            adapters: vec![],
        }
    }
    pub fn register_adapter<T: Adapter>(&mut self, adapter: T) {
        self.adapters.push(Arc::new(Box::new(adapter)));
    }
    pub async fn run_service(self) {
        async fn local(
            db: &Database,
            window: Duration,
            adapter: &Arc<Box<dyn Adapter>>,
        ) -> Result<()> {
            let pending = db.get_pending(Some(window), Some(adapter.name())).await?;

            // Notify adapter about escalation
            for alert in &pending.alerts {
                let level_idx = alert.level_idx(adapter.name());

                if level_idx > 0 {
                    warn!("Escalation occured for alert Id {}", alert.id);
                }

                adapter
                    .notify(
                        Notification::Alert {
                            context: alert.clone(),
                        },
                        level_idx,
                    )
                    .await?;

                info!(
                    "Notified {} adapter about escalation ID {}",
                    adapter.name(),
                    alert.id
                );

                db.mark_delivered(alert.id, adapter.name()).await?
            }

            Ok(())
        }

        // Run background tasks that handles user requests.
        self.run_request_handler();

        tokio::spawn(async move {
            loop {
                for adapter in &self.adapters {
                    if let Err(err) = local(&self.db, self.window, adapter).await {
                        error!(
                            "Error when processing possible escalations for the {} adapter: {:?}",
                            adapter.name(),
                            err
                        );
                    }
                }

                sleep(Duration::from_secs(INTERVAL)).await;
            }
        });
    }
    fn run_request_handler(&self) {
        for adapter in &self.adapters {
            let adapter = Arc::clone(adapter);
            let db = self.db.clone();

            // TODO: Rename variable.
            let others: Vec<Arc<Box<dyn Adapter>>> = self
                .adapters
                .iter()
                // TODO: Remove:
                //.filter(|other| other.name() != adapter.name())
                .map(Arc::clone)
                .collect();

            let adapter_name = adapter.name();
            tokio::spawn(async move {
                // Continue fetching any messages received on the adapter,
                // forever.
                while let Some(action) = adapter.endpoint_request().await {
                    let message = match action.command {
                        Command::Ack(alert_id) => match db
                            .acknowledge_alert(
                                &alert_id,
                                &action.user,
                                adapter_name,
                                action.channel_id,
                                action.is_last_channel,
                            )
                            .await
                        {
                            Ok(res) => match res {
                                AcknowlegementResult::Ok => {
                                    info!(
                                        "Alert {} was acknowleged by {:?}!",
                                        alert_id, action.user
                                    );
                                    UserConfirmation::AlertAcknowledged(alert_id)
                                }
                                AcknowlegementResult::AlreadyAcknowleged(user) => {
                                    debug!(
                                        "Alert {} was already acknowleged by {:?}",
                                        alert_id, user
                                    );
                                    UserConfirmation::AlreadyAcknowleged(user)
                                }
                                AcknowlegementResult::OutOfScope => {
                                    debug!(
                                        "Alert {} is out of scope for user {:?}",
                                        alert_id, action.user
                                    );
                                    UserConfirmation::AlertOutOfScope
                                }
                                AcknowlegementResult::NotFound => {
                                    debug!("Alert {} was not found", alert_id);
                                    UserConfirmation::AlertNotFound
                                }
                            },
                            Err(err) => {
                                error!("Failed to acknowledge alert: {:?}", err);
                                UserConfirmation::InternalError
                            }
                        },
                        Command::Pending => match db.get_pending(None, None).await {
                            Ok(pending) => UserConfirmation::PendingAlerts(pending),
                            Err(err) => {
                                error!("Failed to retrieve pending alerts: {:?}", err);
                                UserConfirmation::InternalError
                            }
                        },
                        Command::Help => UserConfirmation::Help,
                    };

                    // If an alert was acknowledged, notify the other adapters about it.
                    if let UserConfirmation::AlertAcknowledged(alert_id) = message {
                        for other in &others {
                            let acked_by = action.user.clone();
                            let other = Arc::clone(other);

                            // TODO: Handle unwrap
                            let other_level_idx =
                                db.get_level_idx(alert_id, other.name()).await.unwrap();

                            // Don't send the notification to the channel that
                            // acknowledged the alert. That channel already gets
                            // a `UserConfirmation::AlertAcknowledged(_)`
                            // message.
                            let acked_on = if other.name() == adapter_name {
                                Some(action.channel_id)
                            } else {
                                None
                            };

                            // Start the notification process in another thread
                            // which will keep retrying in case the process
                            // fails.
                            tokio::spawn(async move {
                                let mut counter = 0;
                                loop {
                                    if let Err(err) = other
                                        .notify(
                                            Notification::Acknowledged {
                                                id: alert_id,
                                                acked_by: acked_by.clone(),
                                                acked_on,
                                            },
                                            other_level_idx,
                                        )
                                        .await
                                    {
                                        error!("Failed to notify {} adapter about acknowledgement of alert {}: {:?}", other.name(), alert_id, err);
                                        debug!("Retrying...");
                                    } else {
                                        // Notification successful, exit...
                                        break;
                                    }

                                    counter += 1;

                                    // Retry max three times, then exit...
                                    if counter <= 3 {
                                        sleep(Duration::from_secs(5 * counter)).await;
                                    } else {
                                        break;
                                    }
                                }
                            });
                        }
                    }

                    // Send the response directly back to the channel that
                    // issued the command.
                    match adapter.respond(message, action.channel_id).await {
                        Ok(_) => {}
                        Err(err) => {
                            error!(
                                "failed to respond to request on {} adapter: {:?}",
                                adapter_name, err
                            );
                        }
                    }
                }
            });
        }
    }
}
