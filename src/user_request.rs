use crate::database::Database;
use crate::escalation::EscalationService;
use crate::primitives::{Acknowledgement, Command, UserAction, UserConfirmation};
use crate::Result;
use actix::prelude::*;

pub struct RequestHandler<T: Actor> {
    escalation_service: Addr<T>,
    db: Database,
}

impl<T: Actor> RequestHandler<T> {
    pub fn new() -> Self {
        unimplemented!()
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

    fn handle(&mut self, msg: UserAction, ctx: &mut Self::Context) -> Self::Result {
        let escalation_service = self.escalation_service.clone();
        let db = self.db.clone();

        let f = async move {
            let res = match msg.command {
                Command::Ack(alert_id) => {
                    // TODO: Handle unwrap
                    // TODO: Consider the case of disabled escalation (it will go into the void).
                    let x = escalation_service
                        .send(Acknowledgement {
                            user: msg.user,
                            channel_id: msg.channel_id,
                            alert_id,
                        })
                        .await;

                    UserConfirmation::AlertAcknowledged(alert_id)
                }
                Command::Pending => {
                    // TODO: Handle unwrap.
                    let pending = db.get_pending(None).await.unwrap();

                    UserConfirmation::PendingAlerts(pending)
                }
                Command::Help => UserConfirmation::Help,
            };

            Ok(res)
        };

        Box::pin(f.into_actor(self))
    }
}
