use crate::Result;
use crate::database::Database;
use crate::processor::{NotifyAlert, Escalation};
use crate::adapter::{MatrixClient, PagerDutyClient};
use crate::primitives::{Acknowledgement, User, Role, UserConfirmation};
use actix::prelude::*;
use matrix_sdk::instant::SystemTime;
use std::time::Duration;
use std::collections::HashMap;

enum AckPermission<'a, T> {
	Users(Vec<&'a User>),
	MinRole(&'a Role),
	Roles(Vec<&'a Role>),
	EscalationLevel(T),
}

pub struct RoleIndex {
	roles: Vec<(Role, Vec<User>)>,
}

impl RoleIndex {
	pub fn user_is_permitted(&self, user: &User, expected: &[Role]) -> bool {
		self.roles
			.iter()
			.filter(|(_, users)| users.contains(user))
			.any(|(role, _)| expected.contains(role))
	}
	pub fn is_above_minimum(&self, min: &Role, user: &User) -> bool {
		let min_idx = self.roles
			.iter()
			.position(|(role, _)| role == min)
			.unwrap();

		self.roles
			.iter()
			.enumerate()
			.filter(|(_, (_, users))| users.contains(&user))
			.find(|(idx, _)| &idx >= min_idx)
			.is_some()
	}
}

pub struct EscalationService<'a, T, P> {
	db: Database,
	window: Duration,
	actor: Addr<T>,
	last: SystemTime,
	is_locked: bool,
	levels: LevelHandler<P>,
	acks: AckPermission<'a, P>,
	roles: &'a RoleIndex,
}

struct LevelHandler<P> {
	levels: Vec<P>,
}

impl<P> LevelHandler<P> {
	pub fn is_above_level(&self, min: &P, check: &P) -> Option<bool> {
		let min_idx = self.levels
			.iter()
			.position(|level| level == min)?;

		let is_above = self.levels
			.iter()
			.enumerate()
			.filter(|(_, level)| level == check)
			.any(|(idx, _)| idx <= min_idx);

		Some(is_above)
	}
}

impl<'a, T, P> Actor for EscalationService<'a, T, P> {
	type Context = Context<Self>;

	fn started(&mut self, ctx: &mut Self::Context) {
		// TODO. Set appropriate duration
		ctx.run_interval(self.window, |actor, ctx| {
			if actor.last.elapsed().unwrap() < actor.window {
				return;
			}

			actor.is_locked = true;
			let db = self.db.clone();

			actix::spawn(async {

				actor.last = SystemTime::now();
				actor.is_locked = false;
			});
		});
	}
}

impl<'a, T, P> Handler<NotifyAlert> for EscalationService<'a, T, P> {
    type Result = ResponseActFuture<Self, ()>;

    fn handle(&mut self, notify: NotifyAlert, _ctx: &mut Self::Context) -> Self::Result {

		unimplemented!()
	}
}

impl <'a, T, P> Handler<Acknowledgement<P>> for EscalationService<'a, T, P> {
    type Result = ResponseActFuture<Self, Result<UserConfirmation>>;

    fn handle(&mut self, ack: Acknowledgement<P>, ctx: &mut Self::Context) -> Self::Result {
		let f = async move {
			match self.acks {
				AckPermission::Users(users) => {
					if users.contains(ack.user) {
						// Acknowledge alert.
						self.db.ack(&ack.alert_id, &ack.user).await?;
						UserConfirmation::AlertAcknowledged(ack.alert_id)
					} else {
						UserConfirmation::NoPermission
					}
				},
				AckPermission::MinRole(min) => {
					if self.roles.is_above_minimum(min, &ack.user) {
						// Acknowledge alert.
						self.db.ack(&ack.alert_id, &ack.user).await?;
						UserConfirmation::AlertAcknowledged(ack.alert_id)
					} else {
						UserConfirmation::NoPermission
					}
				},
				AckPermission::Roles(roles) => {
					if self.roles.user_is_permitted(&ack.user, &roles) {
						// Acknowledge alert.
						self.db.ack(&ack.alert_id, &ack.user).await?;
						UserConfirmation::AlertAcknowledged(ack.alert_id)
					} else {
						UserConfirmation::NoPermission
					}
				}
				AckPermission::EscalationLevel(level) => {
					if self.levels.is_above_level(&level, &level) {
						// Acknowledge alert.
						self.db.ack(&ack.alert_id, &ack.user).await?;
						UserConfirmation::AlertAcknowledged(ack.alert_id)
					} else {
						UserConfirmation::AlertOutOfScope
					}
				}
			}
		};

		Box::pin(f.into_actor(self))
	}
}

async fn handle_pending(db: Database) -> Result<()> {
	let pending = db.get_pending(None).await?;

	unimplemented!()
}
