use std::collections::VecDeque;
use std::sync::Mutex;

use model::hub::Scope;
use model::share::{Audience, ResourceKind};
use serde_json::Value;
use tokio::sync::watch;

use super::*;

#[derive(Debug)]
struct FakeCenter {
    state: watch::Sender<LinkState>,
    status: watch::Sender<Option<Outcome>>,
    calls: Mutex<Vec<Value>>,
    results: Mutex<VecDeque<Result<Value, CallError>>>,
}

impl FakeCenter {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: watch::channel(LinkState::default()).0,
            status: watch::channel(None).0,
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
            .unwrap_or_else(|| Ok(json!({"accepted":[],"rejected":[],"groups":[]})))
    }

    fn share_status(&self) -> watch::Receiver<Option<Outcome>> {
        self.status.subscribe()
    }
}

#[derive(Debug)]
struct FakeBook {
    entries: Mutex<Vec<Published>>,
    changes: watch::Sender<u64>,
    applied: Mutex<Vec<Outcome>>,
}

impl FakeBook {
    fn new(entries: Vec<Published>) -> Arc<Self> {
        Arc::new(Self {
            entries: Mutex::new(entries),
            changes: watch::channel(0).0,
            applied: Mutex::default(),
        })
    }

    fn set(&self, entries: Vec<Published>) {
        *self.entries.lock().expect("entries") = entries;
        self.changes.send_modify(|revision| *revision += 1);
    }

    fn applied(&self) -> Vec<Outcome> {
        self.applied.lock().expect("applied").clone()
    }
}

impl ShareBook for FakeBook {
    fn publication(&self) -> (u64, Vec<Published>) {
        (
            *self.changes.borrow(),
            self.entries.lock().expect("entries").clone(),
        )
    }

    fn changes(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }

    fn apply(&self, outcome: Outcome) {
        self.applied.lock().expect("applied").push(outcome);
    }
}

fn entry(key: &str) -> Published {
    Published {
        resource_kind: ResourceKind::Workspace,
        resource_key: key.to_owned(),
        group_id: Uuid::nil(),
        permission: "operate".to_owned(),
        audience: Audience::Group,
        recipients: Vec::new(),
        display_name: None,
        project_name: None,
        generation: None,
    }
}

async fn attempt(center: &FakeCenter, book: &FakeBook, publisher: &mut Publisher) -> Attempt {
    let state = center.state.borrow().clone();
    publish(center, book, &state, publisher).await
}

#[tokio::test]
async fn publishes_only_to_online_centers_that_support_shares() {
    let center = FakeCenter::new();
    let book = FakeBook::new(vec![entry("ws_1")]);
    let mut publisher = Publisher::default();

    assert_eq!(
        attempt(&center, &book, &mut publisher).await,
        Attempt::Settled
    );
    center.online(Uuid::new_v4(), &["hub.directory.publish"]);
    assert_eq!(
        attempt(&center, &book, &mut publisher).await,
        Attempt::Settled
    );
    assert!(center.calls().is_empty());

    center.online(Uuid::new_v4(), &[PUBLISH_METHOD]);
    assert_eq!(
        attempt(&center, &book, &mut publisher).await,
        Attempt::Settled
    );
    let calls = center.calls();
    assert_eq!(calls.len(), 1);
    assert!(
        calls[0]["revision"]
            .as_u64()
            .is_some_and(|revision| revision > 0)
    );
    assert_eq!(calls[0]["shares"][0]["resourceKind"], "workspace");
    assert_eq!(calls[0]["shares"][0]["resourceKey"], "ws_1");
    assert_eq!(calls[0]["shares"][0]["groupId"], json!(Uuid::nil()));
    assert_eq!(calls[0]["shares"][0]["audience"], "group");
    assert_eq!(book.applied().len(), 1);
}

#[tokio::test]
async fn fresh_links_republish_the_same_revision_and_changes_get_a_larger_one() {
    let center = FakeCenter::new();
    let book = FakeBook::new(Vec::new());
    let mut publisher = Publisher::default();
    center.online(Uuid::new_v4(), &[PUBLISH_METHOD]);
    attempt(&center, &book, &mut publisher).await;
    // Nothing changed on the same link: no call.
    attempt(&center, &book, &mut publisher).await;
    center.online(Uuid::new_v4(), &[PUBLISH_METHOD]);
    attempt(&center, &book, &mut publisher).await;
    book.set(vec![entry("ws_1")]);
    attempt(&center, &book, &mut publisher).await;

    let revisions: Vec<u64> = center
        .calls()
        .iter()
        .map(|call| call["revision"].as_u64().expect("revision"))
        .collect();
    assert_eq!(revisions.len(), 3);
    assert_eq!(revisions[0], revisions[1]);
    assert!(revisions[2] > revisions[1]);
}

#[tokio::test]
async fn conflicts_publish_anew_and_failures_retry() {
    let center = FakeCenter::new();
    let book = FakeBook::new(vec![entry("ws_1")]);
    let mut publisher = Publisher::default();
    center.online(Uuid::new_v4(), &[PUBLISH_METHOD]);
    center.results.lock().expect("results").extend([
        Err(CallError::Rejected("conflict".to_owned())),
        Err(CallError::Capacity),
        Ok(json!({"unexpected":true})),
        Err(CallError::Unavailable),
    ]);

    assert_eq!(
        attempt(&center, &book, &mut publisher).await,
        Attempt::Retry
    );
    assert_eq!(
        attempt(&center, &book, &mut publisher).await,
        Attempt::Retry
    );
    assert_eq!(
        attempt(&center, &book, &mut publisher).await,
        Attempt::Retry
    );
    assert_eq!(
        attempt(&center, &book, &mut publisher).await,
        Attempt::Settled
    );
    assert_eq!(
        attempt(&center, &book, &mut publisher).await,
        Attempt::Settled
    );

    let revisions: Vec<u64> = center
        .calls()
        .iter()
        .map(|call| call["revision"].as_u64().expect("revision"))
        .collect();
    assert!(
        revisions[1] > revisions[0],
        "a conflict must not reuse the revision"
    );
    assert_eq!(book.applied().len(), 1);
}

#[tokio::test]
async fn the_loop_publishes_and_applies_center_initiated_outcomes() {
    let center = FakeCenter::new();
    let book = FakeBook::new(vec![entry("ws_1")]);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(run(center.clone(), book.clone(), cancel.clone()));
    center.online(Uuid::new_v4(), &[PUBLISH_METHOD]);
    wait(|| book.applied().len() == 1).await;

    center.status.send_replace(Some(Outcome {
        revision: Some(1),
        ..Outcome::default()
    }));
    wait(|| book.applied().len() == 2).await;
    assert_eq!(book.applied()[1].revision, Some(1));

    book.set(vec![entry("ws_1"), entry("ws_2")]);
    wait(|| center.calls().len() == 2).await;
    assert_eq!(
        center.calls()[1]["shares"].as_array().map(Vec::len),
        Some(2)
    );

    cancel.cancel();
    task.await.expect("loop ends");
}

async fn wait(done: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !done() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("condition in time");
}
