use std::collections::VecDeque;
use std::sync::Mutex;

use model::hub::Scope;
use serde_json::json;

use super::*;

#[derive(Debug)]
struct FakeCenter {
    state: watch::Sender<LinkState>,
    calls: Mutex<Vec<Value>>,
    results: Mutex<VecDeque<Result<Value, CallError>>>,
}

impl FakeCenter {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: watch::channel(LinkState::default()).0,
            calls: Mutex::default(),
            results: Mutex::default(),
        })
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

    fn calls(&self) -> Vec<Value> {
        self.calls.lock().expect("calls").clone()
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
        self.calls.lock().expect("calls").push(params);
        self.results
            .lock()
            .expect("results")
            .pop_front()
            .unwrap_or_else(|| Ok(json!({"enabled":true})))
    }
}

fn bot(state: &'static str) -> HubBot {
    HubBot {
        workspace_id: "wks_0123456789abcdef".to_owned(),
        channel_id: serde_json::from_value(json!("00000000-0000-4000-8000-000000000001"))
            .expect("channel ID"),
        title: "app".to_owned(),
        state,
    }
}

fn enabled() -> Value {
    publication(&[bot("active")])
}

#[test]
fn publication_lists_reachable_bots() {
    let body = enabled();
    assert_eq!(body["version"], 2);
    assert_eq!(body["enabled"], true);
    assert_eq!(body["bots"][0]["workspaceId"], "wks_0123456789abcdef");
    assert_eq!(body["bots"][0]["title"], "app");
    assert_eq!(body["bots"][0]["state"], "active");
    assert!(body["bots"][0]["channelId"].is_string());
    let paused = publication(&[bot("paused")]);
    assert_eq!(
        paused["enabled"], false,
        "a paused Bot cannot take messages"
    );
    assert_eq!(paused["bots"][0]["state"], "paused");
    let many: Vec<_> = (0..70).map(|_| bot("active")).collect();
    assert_eq!(
        publication(&many)["bots"].as_array().expect("bots").len(),
        64
    );
}

#[tokio::test]
async fn publishes_once_per_link_and_content() {
    let center = FakeCenter::new();
    let link = Uuid::new_v4();
    center.online(link, &[PUBLISH_METHOD]);
    let mut sent = None;
    let state = center.state.borrow().clone();
    assert_eq!(
        publish(center.as_ref(), &state, enabled(), &mut sent).await,
        Attempt::Settled
    );
    assert_eq!(
        publish(center.as_ref(), &state, enabled(), &mut sent).await,
        Attempt::Settled
    );
    assert_eq!(
        center.calls().len(),
        1,
        "unchanged settings on the same link"
    );
    assert_eq!(center.calls()[0]["bots"][0]["title"], "app");
    assert_eq!(center.calls()[0]["version"], 2);

    center.online(Uuid::new_v4(), &[PUBLISH_METHOD]);
    let fresh = center.state.borrow().clone();
    publish(center.as_ref(), &fresh, enabled(), &mut sent).await;
    publish(center.as_ref(), &fresh, publication(&[]), &mut sent).await;
    let calls = center.calls();
    assert_eq!(calls.len(), 3, "a fresh link and a change both republish");
    assert_eq!(calls[2]["enabled"], false);
}

#[tokio::test]
async fn skips_old_centers_and_offline_links_and_retries_failures() {
    let center = FakeCenter::new();
    let mut sent = None;
    let offline = center.state.borrow().clone();
    assert_eq!(
        publish(center.as_ref(), &offline, enabled(), &mut sent).await,
        Attempt::Settled
    );
    center.online(Uuid::new_v4(), &["hub.shares.publish"]);
    let old = center.state.borrow().clone();
    assert_eq!(
        publish(center.as_ref(), &old, enabled(), &mut sent).await,
        Attempt::Settled
    );
    assert!(center.calls().is_empty());

    center.online(Uuid::new_v4(), &[PUBLISH_METHOD]);
    let state = center.state.borrow().clone();
    center
        .results
        .lock()
        .expect("results")
        .push_back(Err(CallError::Rejected("internal".to_owned())));
    assert_eq!(
        publish(center.as_ref(), &state, enabled(), &mut sent).await,
        Attempt::Retry
    );
    assert_eq!(
        publish(center.as_ref(), &state, enabled(), &mut sent).await,
        Attempt::Settled
    );
    assert_eq!(center.calls().len(), 2);
}

#[tokio::test]
async fn run_republishes_after_a_local_change() {
    let center = FakeCenter::new();
    center.online(Uuid::new_v4(), &[PUBLISH_METHOD]);
    let (bots, receiver) = watch::channel(Vec::new());
    let cancel = CancellationToken::new();
    let task = tokio::spawn(run(center.clone(), receiver, cancel.clone()));
    tokio::time::timeout(Duration::from_secs(3), async {
        while center.calls().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("first publication");
    bots.send_replace(vec![bot("active")]);
    tokio::time::timeout(Duration::from_secs(3), async {
        while center.calls().len() < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("republication");
    assert_eq!(center.calls()[1]["enabled"], true);
    cancel.cancel();
    task.await.expect("publisher stops");
}
