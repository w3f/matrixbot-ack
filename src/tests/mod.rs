use crate::primitives::AlertDelivery;
use crate::Result;
use actix::prelude::*;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

mod escalation;
mod user_request;

struct MockAdapter {
    queue: UnboundedSender<AlertDelivery>,
}

impl MockAdapter {
    fn new() -> (Self, UnboundedReceiver<AlertDelivery>) {
        let (tx, recv) = unbounded_channel();

        (MockAdapter { queue: tx }, recv)
    }
}

impl Actor for MockAdapter {
    type Context = Context<Self>;
}

impl Handler<AlertDelivery> for MockAdapter {
    type Result = Result<()>;

    fn handle(&mut self, msg: AlertDelivery, _ctx: &mut Self::Context) -> Self::Result {
        self.queue.send(msg).map_err(|err| err.into())
    }
}
