use crate::database::Database;
use crate::primitives::{Acknowledgement, Command, UserAction, UserConfirmation};
use crate::Result;
use actix::prelude::*;

pub struct RequestHandler<T: Actor> {
    escalation_service: Addr<T>,
    db: Database,
}

impl<T: Actor> RequestHandler<T> {
    pub fn new(escalation_service: Addr<T>, db: Database) -> Self {
        RequestHandler {
            escalation_service,
            db,
        }
    }
}

impl<T: Actor> Actor for RequestHandler<T> {
    type Context = Context<Self>;
}

impl<T> Handler<UserAction> for RequestHandler<T>
where
    T: Actor + Handler<Acknowledgement>,
    <T as Actor>::Context: actix::dev::ToEnvelope<T, Acknowledgement>,
{
    type Result = ResponseActFuture<Self, Result<UserConfirmation>>;

    fn handle(&mut self, msg: UserAction, _ctx: &mut Self::Context) -> Self::Result {
        let escalation_service = self.escalation_service.clone();
        let db = self.db.clone();

        let f = async move {
            let res = match msg.command {
                Command::Ack(alert_id) => {
                    let res = escalation_service
                        .send(Acknowledgement {
                            user: msg.user,
                            channel_id: msg.channel_id,
                            alert_id,
                        })
                        .await;

                    match res {
                        Ok(resp) => match resp {
                            Ok(confirmation) => confirmation,
                            Err(err) => {
                                error!(
                                    "failed to acknowledge alert {}, error: {:?}",
                                    alert_id, err
                                );
                                UserConfirmation::InternalError
                            }
                        },
                        Err(err) => {
                            error!(
                                "actor mailbox error when attempting to notify about new alert: {:?}",
                                err
                            );
                            UserConfirmation::InternalError
                        }
                    }
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

            Ok(res)
        };

        Box::pin(f.into_actor(self))
    }
}
