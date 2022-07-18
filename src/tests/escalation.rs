use futures::Future;

use super::*;
use crate::{
    escalation::EscalationService,
    primitives::{AlertContext, AlertId, Command, User},
};
use futures::join;
use tokio::time::{sleep, Duration};

async fn wait_for_alerts(amount: usize, comms: &Comms) {
    wait_for_alerts_with_start(0, amount, comms).await
}

async fn wait_for_alerts_with_start(from: usize, to: usize, comms: &Comms) {
    for expected in from..to {
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
                // Make sure that for any notification, the alert is the same,
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

async fn ensure_empty<F: Future<Output = U>, U: std::fmt::Debug>(f: F) -> bool {
    tokio::time::timeout(Duration::from_secs(ESCALATION_WINDOW * 3), f)
        .await
        .is_err()
}

#[tokio::test]
async fn acknowledge_alert_with_repeated_attempt() {
    let (_db, mocker1, mocker2) = setup_mockers().await;

    join!(wait_for_alerts(1, &mocker1), wait_for_alerts(1, &mocker2),);

    // Acknowledge alert.
    mocker1
        .inject(UserAction {
            user: User::FirstMocker,
            channel_id: 3,
            is_last_channel: false,
            command: Command::Ack(AlertId::from(1)),
        })
        .await;

    // Acknowledge alert again, which leaves the state unchanged and
    // generates an extra response.
    mocker1
        .inject(UserAction {
            user: User::FirstMocker,
            channel_id: 3,
            is_last_channel: false,
            command: Command::Ack(AlertId::from(1)),
        })
        .await;

    // Check responses.
    let (confirmation, level) = mocker1.next_response().await;
    match confirmation {
        UserConfirmation::AlertAcknowledged(id) => {
            assert_eq!(id, AlertId::from(1));
            assert_eq!(level, 3);
        }
        _ => {
            dbg!(confirmation);
            panic!();
        }
    }

    let (confirmation, level) = mocker1.next_response().await;
    match confirmation {
        UserConfirmation::AlreadyAcknowleged(user) => {
            assert_eq!(user, User::FirstMocker);
            assert_eq!(level, 3);
        }
        _ => {
            dbg!(confirmation);
            panic!();
        }
    }

    // Mocker2 must be informed about the acknowlegement of the alert.
    let (notification, level) = mocker2.next_notification().await;
    match notification {
        Notification::Acknowledged { id, acked_by } => {
            dbg!(&id);
            dbg!(&acked_by);
            dbg!(&level);

            assert_eq!(id, AlertId::from(1));
            assert_eq!(acked_by, User::FirstMocker);
            // Mocker2 gets notified on level zero (first level), not three (!).
            assert_eq!(level, 0);
        }
        _ => {
            dbg!(notification);
            panic!();
        }
    }

    // No other notifications or responses left.
    let res = join!(
        ensure_empty(mocker1.next_notification()),
        ensure_empty(mocker2.next_notification()),
        ensure_empty(mocker1.next_response()),
        ensure_empty(mocker2.next_response()),
    );

    assert_eq!(res, (true, true, true, true));
}

#[tokio::test]
async fn acknowledge_alert_out_of_scope_with_cross_ack() {
    let (_db, mocker1, mocker2) = setup_mockers().await;

    // Alert notification gets sent three times.
    join!(wait_for_alerts(3, &mocker1), wait_for_alerts(3, &mocker2));

    // Acknowledge alert (invalid attempt).
    mocker1
        .inject(UserAction {
            user: User::FirstMocker,
            // Escalation is on level three, while here we inject a message from
            // level two.
            channel_id: 2,
            is_last_channel: false,
            command: Command::Ack(AlertId::from(1)),
        })
        .await;

    let (confirmation, level) = mocker1.next_response().await;
    match confirmation {
        UserConfirmation::AlertOutOfScope => {
            // Ok.
            assert_eq!(level, 2);
        }
        _ => {
            dbg!(confirmation);
            panic!();
        }
    }

    // Escalations continue...
    join!(
        wait_for_alerts_with_start(3, 6, &mocker1),
        wait_for_alerts_with_start(3, 6, &mocker2)
    );

    // Acknowledge with above level (from different mocker)
    mocker2
        .inject(UserAction {
            user: User::SecondMocker,
            // This was sent from level eight, while the escalation level is at
            // six (3 + 3).
            channel_id: 8,
            is_last_channel: false,
            command: Command::Ack(AlertId::from(1)),
        })
        .await;

    let (confirmation, level) = mocker2.next_response().await;
    match confirmation {
        UserConfirmation::AlertAcknowledged(id) => {
            assert_eq!(id, AlertId::from(1));
            // Gets notified on level eight.
            assert_eq!(level, 8);
        }
        _ => {
            dbg!(confirmation);
            panic!();
        }
    }

    // Mocker1 must be notified about the acknowledgement.
    let (notification, level) = mocker1.next_notification().await;
    match notification {
        Notification::Acknowledged { id, acked_by } => {
            dbg!(&id);
            dbg!(&acked_by);
            dbg!(&level);

            assert_eq!(id, AlertId::from(1));
            assert_eq!(acked_by, User::SecondMocker);
            // Gets notified on level five (6-1), not eight (!).
            assert_eq!(level, 5);
        }
        _ => {
            dbg!(notification);
            panic!();
        }
    }

    // No other *responses* left.
    let res = join!(
        ensure_empty(mocker1.next_notification()),
        ensure_empty(mocker2.next_notification()),
        ensure_empty(mocker1.next_response()),
        ensure_empty(mocker2.next_response()),
    );

    assert_eq!(res, (true, true, true, true));
}

#[tokio::test]
async fn acknowledge_alert_not_found() {
    let (_db, mocker1, mocker2) = setup_mockers().await;

    join!(wait_for_alerts(1, &mocker1), wait_for_alerts(1, &mocker2),);

    // Acknowledge alert (invalid attempt).
    mocker1
        .inject(UserAction {
            user: User::FirstMocker,
            channel_id: 3,
            is_last_channel: false,
            // Alert Id does not exist.
            command: Command::Ack(AlertId::from(10)),
        })
        .await;

    let (confirmation, level) = mocker1.next_response().await;
    match confirmation {
        UserConfirmation::AlertNotFound => {
            // Ok.
            assert_eq!(level, 3);
        }
        _ => {
            dbg!(confirmation);
            panic!();
        }
    }

    // No other *responses* left.
    let res = join!(
        ensure_empty(mocker1.next_notification()),
        ensure_empty(mocker2.next_notification()),
        ensure_empty(mocker1.next_response()),
        ensure_empty(mocker2.next_response()),
    );

    // Escalations continue...
    assert_eq!(res, (false, false, true, true));
}
