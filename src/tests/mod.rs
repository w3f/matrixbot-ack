use crate::adapter::{Adapter, AdapterName};
use crate::primitives::{Notification, UserAction, UserConfirmation};
use crate::Result;
use std::sync::Arc;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::Mutex;

struct Comms {
    notifications: UnboundedReceiver<(Notification, usize)>,
    responses: UnboundedReceiver<(UserConfirmation, usize)>,
    injector: UnboundedSender<UserAction>,
}

struct FirstMocker {
    notifications: UnboundedSender<(Notification, usize)>,
    responses: UnboundedSender<(UserConfirmation, usize)>,
    injector: Arc<Mutex<UnboundedReceiver<UserAction>>>,
}

impl FirstMocker {
    pub fn new() -> (Self, Comms) {
        let (tx1, recv1) = unbounded_channel();
        let (tx2, recv2) = unbounded_channel();
        let (tx3, recv3) = unbounded_channel();

        (
            FirstMocker {
                notifications: tx1,
                responses: tx2,
                injector: Arc::new(Mutex::new(recv3)),
            },
            Comms {
                notifications: recv1,
                responses: recv2,
                injector: tx3,
            },
        )
    }
}

#[async_trait]
impl Adapter for FirstMocker {
    fn name(&self) -> AdapterName {
        AdapterName::MockerFirst
    }
    async fn notify(&self, notification: Notification, level_idx: usize) -> Result<()> {
        self.notifications
            .send((notification, level_idx))
            .map_err(|err| err.into())
    }
    async fn respond(&self, resp: UserConfirmation, level_idx: usize) -> Result<()> {
        self.responses
            .send((resp, level_idx))
            .map_err(|err| err.into())
    }
    async fn endpoint_request(&self) -> Option<UserAction> {
        let mut l = self.injector.lock().await;
        l.recv().await
    }
}

struct SecondMocker {
    notifications: UnboundedSender<(Notification, usize)>,
    responses: UnboundedSender<(UserConfirmation, usize)>,
    injector: Arc<Mutex<UnboundedReceiver<UserAction>>>,
}

impl SecondMocker {
    pub fn new() -> (Self, Comms) {
        let (tx1, recv1) = unbounded_channel();
        let (tx2, recv2) = unbounded_channel();
        let (tx3, recv3) = unbounded_channel();

        (
            SecondMocker {
                notifications: tx1,
                responses: tx2,
                injector: Arc::new(Mutex::new(recv3)),
            },
            Comms {
                notifications: recv1,
                responses: recv2,
                injector: tx3,
            },
        )
    }
}

#[async_trait]
impl Adapter for SecondMocker {
    fn name(&self) -> AdapterName {
        AdapterName::MockerSecond
    }
    async fn notify(&self, notification: Notification, level_idx: usize) -> Result<()> {
        self.notifications
            .send((notification, level_idx))
            .map_err(|err| err.into())
    }
    async fn respond(&self, resp: UserConfirmation, level_idx: usize) -> Result<()> {
        self.responses
            .send((resp, level_idx))
            .map_err(|err| err.into())
    }
    async fn endpoint_request(&self) -> Option<UserAction> {
        let mut l = self.injector.lock().await;
        l.recv().await
    }
}
