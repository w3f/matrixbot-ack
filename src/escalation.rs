use crate::Result;
use crate::database::Database;
use crate::processor::{NotifyAlert, Escalation};
use crate::adapter::{MatrixClient, PagerDutyClient};
use actix::prelude::*;
use matrix_sdk::instant::SystemTime;
use std::time::Duration;

struct User;
struct Role;

enum AckPermission {
	Users(Vec<User>),
	MinRole(Role),
	Roles(Vec<Role>),
}

pub struct EscalationService<T: Actor> {
	db: Database,
	window: Duration,
	actor: Addr<T>,
	last: SystemTime,
	acks: AckPermission,
}

impl<T: Actor> Actor for EscalationService<T> {
	type Context = Context<Self>;

	fn started(&mut self, ctx: &mut Self::Context) {
		// TODO. Set appropriate duration
		ctx.run_interval(self.window, |actor, ctx| {
			if self.last.elapsed().unwrap() < self.window {
				return;
			}

			let db = self.db.clone();
			actix::spawn(async {

			});

			self.last = SystemTime::now();
		});
	}
}

impl<T: Actor Handler<NotifyAlert> for EscalationService<T> {
    type Result = ResponseActFuture<Self, ()>;

    fn handle(&mut self, notify: NotifyAlert, _ctx: &mut Self::Context) -> Self::Result {


		unimplemented!()
	}
}

async fn handle_pending(db: Database) -> Result<()> {
	let pending = db.get_pending(None).await?;

	unimplemented!()
}
