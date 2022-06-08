use crate::Result;
use crate::primitives::{UserAction, UserConfirmation, Command};
use actix::prelude::*;

pub struct RequestHandler {}

impl Actor for RequestHandler {
    type Context = Context<Self>;
}

impl Handler<UserAction> for RequestHandler {
    type Result = ResponseActFuture<Self, Result<UserConfirmation>>;

    fn handle(&mut self, msg: UserAction, ctx: &mut Self::Context) -> Self::Result {
        let f = async move {
            match msg.command {
                Command::Ack(alert_id) => {

                },
                Command::Pending => {

                },
                Command::Help => {

                },
            }

            unimplemented!()
        };

        Box::pin(f.into_actor(self))
    }
}
