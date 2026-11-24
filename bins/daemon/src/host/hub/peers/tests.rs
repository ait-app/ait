use std::collections::VecDeque;
use std::sync::Mutex;

use model::hub::Scope;
use serde_json::json;
use tokio::sync::mpsc;

use super::*;

/// One call the center received, with the logical link it arrived on.
type Received = (Option<Uuid>, Value);

#[derive(Debug)]
struct FakeCenter {
    state: watch::Sender<LinkState>,
    calls: mpsc::UnboundedSender<Received>,
    results: Mutex<VecDeque<Result<Value, CallError>>>,
}

impl FakeCenter {
    fn new() -> (Arc<Self>, mpsc::UnboundedReceiver<Received>) {
        let (calls, received) = mpsc::unbounded_channel();
        let center = Arc::new(Self {
            state: watch::channel(LinkState::default()).0,
            calls,
            results: Mutex::default(),
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

    fn fail_next(&self, error: CallError) {
        self.results.lock().expect("results").push_back(Err(error));
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
        self.results
            .lock()
            .expect("results")
            .pop_front()
            .unwrap_or_else(|| Ok(json!({"enabled": enabled})))
    }
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

    center.online(first, &[PUBLISH_METHOD]);
    switch.send_replace(true);
    let second = Uuid::new_v4();
    center.online(second, &[PUBLISH_METHOD]);
    assert_eq!(
        next(&mut received).await,
        (Some(second), json!({"version":1,"enabled":true})),
        "an unchanged value on the same link is not published again; a fresh link is"
    );

    switch.send_replace(false);
    assert_eq!(
        next(&mut received).await,
        (Some(second), json!({"version":1,"enabled":false}))
    );
    cancel.cancel();
    task.await.expect("publisher stops");
    assert!(received.try_recv().is_err(), "no further publication");

    let mut sent = None;
    let unchanged = center.state.borrow().clone();
    assert_eq!(
        publish(center.as_ref(), &unchanged, false, &mut sent).await,
        Attempt::Settled
    );
    assert_eq!(
        publish(center.as_ref(), &unchanged, false, &mut sent).await,
        Attempt::Settled
    );
    assert_eq!(next(&mut received).await.1["enabled"], false);
    assert!(
        received.try_recv().is_err(),
        "same link and value publish once"
    );

    center.online(Uuid::new_v4(), &["hub.shares.publish"]);
    let old = center.state.borrow().clone();
    assert_eq!(
        publish(center.as_ref(), &old, true, &mut sent).await,
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
    let offline = center.state.borrow().clone();
    assert_eq!(
        publish(center.as_ref(), &offline, true, &mut sent).await,
        Attempt::Settled
    );
    assert!(
        received.try_recv().is_err(),
        "an offline link is not called"
    );

    center.online(Uuid::new_v4(), &[PUBLISH_METHOD]);
    let state = center.state.borrow().clone();
    for settled in [
        CallError::Rejected("forbidden".to_owned()),
        CallError::Unsupported,
        CallError::Unavailable,
    ] {
        center.fail_next(settled.clone());
        assert_eq!(
            publish(center.as_ref(), &state, true, &mut sent).await,
            Attempt::Settled,
            "{settled}"
        );
        next(&mut received).await;
    }
    center.fail_next(CallError::Rejected("internal".to_owned()));
    assert_eq!(
        publish(center.as_ref(), &state, true, &mut sent).await,
        Attempt::Retry
    );
    assert_eq!(
        publish(center.as_ref(), &state, true, &mut sent).await,
        Attempt::Settled
    );
    assert_eq!(
        publish(center.as_ref(), &state, true, &mut sent).await,
        Attempt::Settled
    );
    next(&mut received).await;
    next(&mut received).await;
    assert!(
        received.try_recv().is_err(),
        "a confirmed publication is not repeated"
    );
}
