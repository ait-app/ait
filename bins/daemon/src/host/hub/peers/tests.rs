use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use model::hub::{LinkState, Scope};
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

use super::*;

/// One call the center received, with the logical link it arrived on.
type Received = (Option<Uuid>, Value);

/// A scripted failure, and whether the link goes offline while the call is out.
type Failure = (CallError, bool);

#[derive(Debug)]
struct FakeCenter {
    state: watch::Sender<LinkState>,
    calls: mpsc::UnboundedSender<Received>,
    failures: Mutex<VecDeque<Failure>>,
}

impl FakeCenter {
    fn new() -> (Arc<Self>, mpsc::UnboundedReceiver<Received>) {
        let (calls, received) = mpsc::unbounded_channel();
        let center = Arc::new(Self {
            state: watch::channel(LinkState::default()).0,
            calls,
            failures: Mutex::default(),
        });
        (center, received)
    }

    fn online(&self, link: Uuid, methods: &[&str]) {
        self.state.send_replace(LinkState {
            scope: Some(Scope {
                center: "https://center.test".to_owned(),
                node_session_id: Uuid::nil(),
            }),
            online: true,
            link: Some(link),
            methods: methods.iter().map(|m| (*m).to_owned()).collect(),
        });
    }

    fn link(&self) -> LinkState {
        self.state.borrow().clone()
    }

    fn fail_next(&self, error: CallError, link_drops: bool) {
        self.failures
            .lock()
            .expect("failures")
            .push_back((error, link_drops));
    }
}

#[async_trait::async_trait]
impl Center for FakeCenter {
    fn state(&self) -> watch::Receiver<LinkState> {
        self.state.subscribe()
    }

    async fn call(
        &self,
        _scope: Scope,
        method: &'static str,
        params: Value,
    ) -> Result<Value, CallError> {
        assert_eq!(method, PUBLISH_METHOD);
        let enabled = params["enabled"].clone();
        self.calls
            .send((self.state.borrow().link, params))
            .expect("test receives calls");
        let failure = self.failures.lock().expect("failures").pop_front();
        match failure {
            Some((error, link_drops)) => {
                if link_drops {
                    self.state.send_modify(|state| state.online = false);
                }
                Err(error)
            }
            None => Ok(json!({"enabled": enabled})),
        }
    }
}

async fn publish(
    center: &FakeCenter,
    link: &LinkState,
    enabled: bool,
    sent: &mut publisher::Sent,
) -> Attempt {
    publisher::publish(&PUBLICATION, center, link, body(enabled), sent).await
}

async fn next(received: &mut mpsc::UnboundedReceiver<Received>) -> Received {
    tokio::time::timeout(Duration::from_secs(5), received.recv())
        .await
        .expect("a publication within the guard")
        .expect("center still observed")
}

#[tokio::test]
async fn publication_follows_the_link_and_the_switch() {
    let (center, mut received) = FakeCenter::new();
    let (switch, accepting) = watch::channel(false);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(run(center.clone(), accepting, cancel.clone()));

    center.online(Uuid::new_v4(), &["hub.shares.publish"]);
    let first = Uuid::new_v4();
    center.online(first, &[PUBLISH_METHOD]);
    assert_eq!(
        next(&mut received).await,
        (Some(first), json!({"version":1,"enabled":false})),
        "a fresh link publishes even when not accepting, and an old center gets nothing"
    );

    switch.send_replace(true);
    assert_eq!(
        next(&mut received).await,
        (Some(first), json!({"version":1,"enabled":true}))
    );

    let second = Uuid::new_v4();
    center.online(second, &[PUBLISH_METHOD]);
    assert_eq!(
        next(&mut received).await,
        (Some(second), json!({"version":1,"enabled":true})),
        "a fresh link publishes an unchanged value again"
    );

    switch.send_replace(false);
    assert_eq!(
        next(&mut received).await,
        (Some(second), json!({"version":1,"enabled":false}))
    );
    cancel.cancel();
    task.await.expect("publisher stops");
    assert!(received.try_recv().is_err(), "no further publication");
}

#[tokio::test]
async fn publishes_once_per_link_and_value_and_skips_old_centers() {
    let (center, mut received) = FakeCenter::new();
    let mut sent = None;
    center.online(Uuid::new_v4(), &[PUBLISH_METHOD]);
    let link = center.link();
    for _ in 0..2 {
        assert_eq!(
            publish(&center, &link, false, &mut sent).await,
            Attempt::Settled
        );
    }
    assert_eq!(next(&mut received).await.1["enabled"], false);
    assert!(
        received.try_recv().is_err(),
        "same link and value publish once"
    );

    assert_eq!(
        publish(&center, &link, true, &mut sent).await,
        Attempt::Settled
    );
    assert_eq!(next(&mut received).await.1["enabled"], true);

    center.online(Uuid::new_v4(), &["hub.shares.publish"]);
    let old = center.link();
    assert_eq!(
        publish(&center, &old, false, &mut sent).await,
        Attempt::Settled
    );
    assert!(
        received.try_recv().is_err(),
        "a center without the method is not called"
    );
}

#[tokio::test]
async fn refusals_settle_and_other_failures_retry() {
    let (center, mut received) = FakeCenter::new();
    let mut sent = None;
    let offline = center.link();
    assert_eq!(
        publish(&center, &offline, true, &mut sent).await,
        Attempt::Settled
    );
    assert!(
        received.try_recv().is_err(),
        "an offline link is not called"
    );

    center.online(Uuid::new_v4(), &[PUBLISH_METHOD]);
    let link = center.link();
    for settled in [
        CallError::Rejected("forbidden".to_owned()),
        CallError::Unsupported,
    ] {
        center.fail_next(settled.clone(), false);
        assert_eq!(
            publish(&center, &link, true, &mut sent).await,
            Attempt::Settled,
            "{settled}"
        );
        next(&mut received).await;
    }
    for retried in [
        CallError::Unavailable,
        CallError::Capacity,
        CallError::Protocol,
        CallError::Rejected("internal".to_owned()),
    ] {
        center.fail_next(retried.clone(), false);
        assert_eq!(
            publish(&center, &link, true, &mut sent).await,
            Attempt::Retry,
            "{retried} on an unchanged online link"
        );
        next(&mut received).await;
    }
    for _ in 0..2 {
        assert_eq!(
            publish(&center, &link, true, &mut sent).await,
            Attempt::Settled
        );
    }
    next(&mut received).await;
    assert!(
        received.try_recv().is_err(),
        "a confirmed publication is not repeated"
    );
}

#[tokio::test]
async fn an_unavailable_link_that_is_gone_settles() {
    let (center, mut received) = FakeCenter::new();
    let mut sent = None;
    center.online(Uuid::new_v4(), &[PUBLISH_METHOD]);
    let link = center.link();
    center.fail_next(CallError::Unavailable, true);
    assert_eq!(
        publish(&center, &link, true, &mut sent).await,
        Attempt::Settled,
        "the next link publishes again"
    );
    next(&mut received).await;

    center.online(Uuid::new_v4(), &[PUBLISH_METHOD]);
    let replaced = center.link();
    center.fail_next(CallError::Unavailable, false);
    center.online(Uuid::new_v4(), &[PUBLISH_METHOD]);
    assert_eq!(
        publish(&center, &replaced, true, &mut sent).await,
        Attempt::Settled,
        "a call on a replaced link settles"
    );
    next(&mut received).await;
}
