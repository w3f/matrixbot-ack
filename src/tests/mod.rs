use crate::escalation::EscalationService;
use crate::primitives::{AlertDelivery, ChannelId, Command, User, UserAction, UserConfirmation};
use crate::user_request::RequestHandler;
use crate::Result;
use actix::prelude::*;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

mod escalation;
mod user_request;

type RequestHandlerAddr = Addr<RequestHandler<EscalationService<MockAdapter>>>;

struct MockAdapter {
    queue: UnboundedSender<AlertDelivery>,
    request_handler: RequestHandlerAddr,
}

impl MockAdapter {
    fn new(request_handler: RequestHandlerAddr) -> (Self, UnboundedReceiver<AlertDelivery>) {
        let (queue, recv) = unbounded_channel();

        (
            MockAdapter {
                queue,
                request_handler,
            },
            recv,
        )
    }
    async fn inject_command(
        &self,
        user: User,
        channel_name: String,
        command: Command,
    ) -> Result<UserConfirmation> {
        self.request_handler
            .send(UserAction {
                user,
                channel_id: ChannelId::Mocker(channel_name),
                command,
            })
            .await
            .unwrap()
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
