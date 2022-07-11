use crate::adapter::Adapter;
use crate::database::Database;
use crate::primitives::{Command, Notification, UserConfirmation};
use crate::Result;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

const INTERVAL: u64 = 10;

pub struct EscalationService {
    db: Database,
    window: Duration,
    adapters: Vec<Arc<Box<dyn Adapter>>>,
}

impl EscalationService {
    async fn register_adapter<T: Adapter>(mut self, adapter: T) -> Self {
        self.adapters.push(Arc::new(Box::new(adapter)));
        self
    }
    async fn run(self) {
        async fn local(
            db: &Database,
            window: Duration,
            adapter: &Arc<Box<dyn Adapter>>,
        ) -> Result<()> {
            let pending = db.get_pending(Some(window), Some(adapter.name())).await?;

            // Notify adapter about escalation
            for alert in &pending.alerts {
                adapter
                    .notify(Notification::Alert {
                        context: alert.clone(),
                    })
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
    }
    fn run_request_handler(&self) {
        for adapter in &self.adapters {
            let adapter = Arc::clone(adapter);
            let db = self.db.clone();

            let others: Vec<Arc<Box<dyn Adapter>>> = self
                .adapters
                .iter()
                .filter(|other| other.name() != adapter.name())
                .map(Arc::clone)
                .collect();

            let adapter_name = adapter.name();
            tokio::spawn(async move {
                while let Some(action) = adapter.endpoint_request().await {
                    let message = match action.command {
                        Command::Ack(alert_id) => match db
                            .acknowledge_alert(&alert_id, &action.user)
                            .await
                        {
                            Ok(_) => {
                                info!("Alert {} was acknowleged by {:?}!", alert_id, action.user);
                                UserConfirmation::AlertAcknowledged(alert_id)
                            }
                            Err(err) => {
                                error!("failed to acknowledge alert: {:?}", err);
                                UserConfirmation::InternalError
                            }
                        },
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
                    if let UserConfirmation::AlertAcknowledged(alert_id) = message {
                        for other in &others {
                            let acked_by = action.user.clone();
                            let other = Arc::clone(other);

                            // Start the notification process in another thread
                            // which will keep retrying in case the process
                            // fails.
                            tokio::spawn(async move {
                                let mut counter = 0;
                                loop {
                                    if let Err(err) = other
                                        .notify(Notification::Acknowledged {
                                            id: alert_id,
                                            acked_by: acked_by.clone(),
                                        })
                                        .await
                                    {
                                        error!("Failed to notify {} adapter about acknowledged of ID {}: {:?}", other.name(), alert_id, err);
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
