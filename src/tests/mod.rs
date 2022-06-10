use crate::database::{Database, DatabaseConfig};
use crate::escalation::EscalationService;
use crate::primitives::{AlertDelivery, ChannelId, Command, User, UserAction, UserConfirmation};
use crate::user_request::RequestHandler;
use crate::{start_tasks, Result};
use actix::prelude::*;
use rand::{thread_rng, Rng};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

mod escalation;
mod user_request;

type RequestHandlerAddr = Addr<RequestHandler<EscalationService<MockAdapter>>>;

/*
Todo: fix this:

RequestHandler -> EscalationService
EscalationService -> Adapter
Adapter -> RequestHandler
*/

async fn init_mocker() -> Vec<MockAdapter> {
    // Setup MongoDb database.
    let random: u32 = thread_rng().gen_range(u32::MIN..u32::MAX);
    let db = Database::new(DatabaseConfig {
        uri: "mongodb://localhost:27017/".to_string(),
        name: format!("registrar_test_{}", random),
    })
    .await
    .unwrap();

    //let x = start_tasks(db, client, escalation_config, role_index, levels)

    unimplemented!()
}

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
