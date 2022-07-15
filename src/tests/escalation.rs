use futures::Future;

use super::*;
use crate::{
    escalation::EscalationService,
    primitives::{AlertContext, AlertId, Command, User},
};
use tokio::time::{sleep, Duration};

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
            _ => {
                dbg!(notification);
                panic!();
            }
        }

        dbg!(level);
        dbg!(expected);

        // Make sure the level increases after each notification.
        assert_eq!(level, expected);
    }
}

async fn ensure_empty<F: Future<Output = U>, U: std::fmt::Debug>(f: F) {
    if let Ok(v) = tokio::time::timeout(Duration::from_secs(ESCALATION_WINDOW * 3), f).await {
        dbg!(v);
        panic!();
    }
}

#[tokio::test]
async fn increase_escalation_levels() {
    let (_db, mut mocker1, mut mocker2) = setup_mockers().await;

    wait_for_alerts(5, &mut mocker1).await;
    wait_for_alerts(5, &mut mocker2).await;
}

#[tokio::test]
async fn acknowledge_alert() {
    let (_db, mut mocker1, mut mocker2) = setup_mockers().await;

    wait_for_alerts(3, &mut mocker1).await;
    // Let mocker2 catchup with the escalation.
    sleep(Duration::from_secs(ESCALATION_WINDOW / 2)).await;

    // Acknowledge alert.
    mocker1
        .inject(UserAction {
            user: User::FirstMocker,
            channel_id: 3,
            command: Command::Ack(AlertId::from(1)),
        })
        .await;

    // Mocker2 must have the same alert notifications as mocker1.
    wait_for_alerts(3, &mut mocker2).await;

    // Mocker2 must be informed about the acknowlegement of the alert.
    let (notification, level) = mocker2.next_notification().await;
    match notification {
        Notification::Acknowledged { id, acked_by } => {
            dbg!(&id);
            dbg!(&acked_by);
            dbg!(&level);

            assert_eq!(id, AlertId::from(1));
            assert_eq!(acked_by, User::FirstMocker);
            assert_eq!(level, 3);
        }
        _ => {
            dbg!(notification);
            panic!();
        }
    }

    // No other notifications left.
    ensure_empty(mocker1.next_notification()).await;
    ensure_empty(mocker2.next_notification()).await;
}
