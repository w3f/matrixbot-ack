use super::*;
use crate::{
    escalation::EscalationService,
    primitives::{AlertContext, AlertId},
};
use std::time::Duration;

async fn wait_for_alerts(amount: usize, comms: &mut Comms) {
	for expected in 0..amount {
		let (notification, level) = comms.next_notification().await;

		let mut alert = None;

		match notification {
			Notification::Alert { context } => {
				// Keep track of the alert when received the first time.
				if alert.is_none() {
					alert = Some(context.alert.clone());
				}

				dbg!(context.id);

				assert_eq!(context.id, AlertId::from(1));
				// Make sure that for any escalations, the alert is the same,
				// content wise.
				assert_eq!(&context.alert, alert.as_ref().unwrap());
			}
			_ => panic!(),
		}

		dbg!(level);
		dbg!(expected);

		// Make sure the level increases after each notification.
		assert_eq!(level, expected);
	}
}

#[tokio::test]
async fn increase_escalation_levels() {
	let (_db, mut mocker1, mut mocker2) = setup_mockers().await;

	wait_for_alerts(5, &mut mocker1).await;
	wait_for_alerts(5, &mut mocker2).await;
}
