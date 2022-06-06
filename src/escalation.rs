use crate::Result;
use crate::database::Database;
use crate::processor::{NotifyAlert, Escalation};
use crate::adapter::{MatrixClient, PagerDutyClient};
use crate::primitives::{Acknowledgement, User, Role, UserConfirmation};
use actix::prelude::*;
use actix_broker::{BrokerSubscribe};
use matrix_sdk::instant::SystemTime;
use std::time::Duration;
use std::collections::HashMap;
use std::sync::Arc;

enum AckPermission<T> {
	Users(Vec<User>),
	MinRole(Role),
	Roles(Vec<Role>),
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

pub struct EscalationService<T: Actor, P> {
	db: Database,
	window: Duration,
	adapter: Addr<T>,
	last: SystemTime,
	is_locked: bool,
	levels: Arc<LevelHandler<P>>,
	acks: Arc<AckPermission<P>>,
	roles: Arc<RoleIndex>,
}

struct LevelHandler<P> {
	levels: Vec<P>,
}

impl<P: Eq> LevelHandler<P> {
	pub fn is_above_level(&self, min: &P, check: &P) -> Option<bool> {
		let min_idx = self.levels
			.iter()
			.position(|level| level == min)?;

		let is_above = self.levels
			.iter()
			.enumerate()
			.filter(|(_, level)| *level == check)
			.any(|(idx, _)| idx <= min_idx);

		Some(is_above)
	}
}

impl<T: Actor, P: 'static + Unpin> Actor for EscalationService<T, P> {
	type Context = Context<Self>;

	fn started(&mut self, ctx: &mut Self::Context) {
		ctx.run_interval(self.window, |actor, ctx| {
			if actor.last.elapsed().unwrap() < actor.window {
				return;
			}

			actor.is_locked = true;
			let db = self.db.clone();

			actix::spawn(async {
				// TODO: Handle unwrap
				/*
				let pending = actor.db.get_pending().await.unwrap();
				for alert in pending {
					let x = actor.adapter.send(alert).await;
				}
				*/

				actor.last = SystemTime::now();
				actor.is_locked = false;
			});
		});
	}
}

impl <T: Actor, P: 'static + Unpin + Eq> Handler<Acknowledgement<P>> for EscalationService<T, P> {
    type Result = ResponseActFuture<Self, Result<UserConfirmation>>;

    fn handle(&mut self, ack: Acknowledgement<P>, ctx: &mut Self::Context) -> Self::Result {
		let db = self.db.clone();
		let roles = Arc::clone(&self.roles);
		let levels = Arc::clone(&self.levels);
		let acks = Arc::clone(&self.acks);

		let f = async move {
			let res = match acks {
				AckPermission::Users(users) => {
					if users.contains(&ack.user) {
						// Acknowledge alert.
						db.ack(&ack.alert_id, &ack.user).await?;
						UserConfirmation::AlertAcknowledged(ack.alert_id)
					} else {
						UserConfirmation::NoPermission
					}
				},
				AckPermission::MinRole(min) => {
					if roles.is_above_minimum(&min, &ack.user) {
						// Acknowledge alert.
						db.ack(&ack.alert_id, &ack.user).await?;
						UserConfirmation::AlertAcknowledged(ack.alert_id)
					} else {
						UserConfirmation::NoPermission
					}
				},
				AckPermission::Roles(roles) => {
					if roles.user_is_permitted(&ack.user, &roles) {
						// Acknowledge alert.
						db.ack(&ack.alert_id, &ack.user).await?;
						UserConfirmation::AlertAcknowledged(ack.alert_id)
					} else {
						UserConfirmation::NoPermission
					}
				}
				AckPermission::EscalationLevel(level) => {
					if levels.is_above_level(&level, &level)
					.ok_or_else(|| anyhow!("TODO"))? {
						// Acknowledge alert.
						db.ack(&ack.alert_id, &ack.user).await?;
						UserConfirmation::AlertAcknowledged(ack.alert_id)
					} else {
						UserConfirmation::AlertOutOfScope
					}
				}
			};

			Ok(res)
		};

		Box::pin(f.into_actor(self))
	}
}
