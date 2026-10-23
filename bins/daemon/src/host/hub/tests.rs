use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};

use model::hub::{CallFuture, OutboxStore, StoreError};
use serde_json::json;
use uuid::Uuid;

use super::*;

mod wire;

#[derive(Debug, Default)]
pub(super) struct MemoryStore {
    pub(super) document: Mutex<Value>,
    pub(super) fail: AtomicBool,
}

impl OutboxStore for MemoryStore {
    fn load(&self) -> Result<Value, StoreError> {
        Ok(self.document.lock().unwrap().clone())
    }
    fn save(&self, value: &Value) -> Result<(), StoreError> {
        if self.fail.load(Ordering::Relaxed) {
            return Err(StoreError);
        }
        *self.document.lock().unwrap() = value.clone();
        Ok(())
    }
}

pub(super) fn scope() -> Scope {
    Scope {
        center: "https://center.example/".to_owned(),
        node_session_id: Uuid::new_v4(),
    }
}

pub(super) fn sources(directory: &std::path::Path) -> Sources {
    use domain::agent_runtime::registry::AgentRuntimeRegistry;
    use model::workspace::registry::{ProjectRegistry, WorkspaceRegistry};
    let projects = persistence::storage::registry::FileBackedProjectRegistry::new(
        directory.join("projects.json"),
    );
    let workspaces = persistence::storage::registry::FileBackedWorkspaceRegistry::new(
        directory.join("workspaces.json"),
    );
    let agents = persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry::new(
        directory.join("agents.json"),
    );
    projects.initialize().unwrap();
    workspaces.initialize().unwrap();
    agents.initialize().unwrap();
    Sources {
        projects: Arc::new(projects),
        workspaces: Arc::new(workspaces),
        agents: Arc::new(agents),
        changes: model::changes::Changes::default(),
    }
}

#[derive(Debug)]
struct FakeCenter {
    state: watch::Sender<LinkState>,
    calls: Mutex<Vec<(Scope, &'static str, Value)>>,
    results: Mutex<VecDeque<Result<Value, CallError>>>,
}

impl FakeCenter {
    fn snapshot(&self) -> LinkState {
        self.state.borrow().clone()
    }
}

impl Center for FakeCenter {
    fn state(&self) -> watch::Receiver<LinkState> {
        self.state.subscribe()
    }
    fn call(&self, scope: Scope, method: &'static str, params: Value) -> CallFuture<'_> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((scope, method, params.clone()));
            self.results.lock().unwrap().pop_front().unwrap_or_else(|| {
                Ok(if method == EVENTS {
                    json!({"cursor":1,"inserted":true})
                } else {
                    json!({"revision":params["revision"],"changed":true})
                })
            })
        })
    }
}

fn application() -> (
    Arc<Application>,
    Arc<FakeCenter>,
    Arc<MemoryStore>,
    tempfile::TempDir,
) {
    let directory = tempfile::tempdir().unwrap();
    let center = Arc::new(FakeCenter {
        state: watch::channel(LinkState {
            scope: Some(scope()),
            online: true,
            link: Some(Uuid::new_v4()),
            methods: vec![EVENTS.to_owned(), DIRECTORY.to_owned()],
        })
        .0,
        calls: Mutex::new(Vec::new()),
        results: Mutex::new(VecDeque::new()),
    });
    let store = Arc::new(MemoryStore::default());
    let app = Arc::new(Application {
        center: center.clone(),
        sources: sources(directory.path()),
        server: "server".to_owned(),
        outbox: Arc::new(Mutex::new(Outbox::open(store.clone()).unwrap())),
        changed: tokio::sync::Notify::new(),
        cancel: CancellationToken::new(),
        native: true,
    });
    (app, center, store, directory)
}

#[tokio::test]
async fn retry_and_restart_use_the_same_event_id_and_business_success_removes_it() {
    let (app, center, store, _directory) = application();
    let scope = center.state.borrow().scope.clone().unwrap();
    app.outbox.lock().unwrap().activate(&scope, 0).unwrap();
    let params = json!({"eventId":Uuid::new_v4(),"method":"status.server_info","params":{}});
    app.outbox
        .lock()
        .unwrap()
        .enqueue(&scope, params.clone(), Utc::now().timestamp_millis())
        .unwrap();
    center
        .results
        .lock()
        .unwrap()
        .push_back(Err(CallError::Rejected("push_capacity".to_owned())));
    assert!(app.attempt(center.snapshot()).await.is_err());
    *app.outbox.lock().unwrap() = Outbox::open(store.clone()).unwrap();
    assert!(app.attempt(center.snapshot()).await.unwrap());
    let calls = center.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].2, params);
    assert_eq!(calls[1].2, params);
    assert!(
        app.outbox
            .lock()
            .unwrap()
            .next(&scope, Utc::now().timestamp_millis())
            .is_none()
    );
}

#[tokio::test]
async fn rejected_events_are_quarantined_and_invalid_replies_remain_pending() {
    let (app, center, _store, _directory) = application();
    let scope = center.state.borrow().scope.clone().unwrap();
    app.outbox.lock().unwrap().activate(&scope, 0).unwrap();
    for _ in 0..2 {
        app.outbox
            .lock()
            .unwrap()
            .enqueue(
                &scope,
                json!({"eventId":Uuid::new_v4(),"method":"status.server_info","params":{}}),
                Utc::now().timestamp_millis(),
            )
            .unwrap();
    }
    center.results.lock().unwrap().extend([
        Err(CallError::Rejected("conflict".to_owned())),
        Ok(json!({})),
    ]);
    assert!(app.attempt(center.snapshot()).await.unwrap());
    assert!(app.attempt(center.snapshot()).await.is_err());
    assert!(app.attempt(center.snapshot()).await.unwrap());
    assert!(
        app.outbox
            .lock()
            .unwrap()
            .next(&scope, Utc::now().timestamp_millis())
            .is_none()
    );
}

#[tokio::test]
async fn reconciliation_publishes_three_directories_and_fresh_link_republishes_same_revisions() {
    let (app, center, _store, _directory) = application();
    app.reconcile(center.snapshot(), true).await.unwrap();
    for _ in 0..3 {
        assert!(app.attempt(center.snapshot()).await.unwrap());
    }
    assert!(!app.attempt(center.snapshot()).await.unwrap());
    app.reconcile(center.snapshot(), false).await.unwrap();
    assert!(!app.attempt(center.snapshot()).await.unwrap());
    app.reconcile(center.snapshot(), true).await.unwrap();
    for _ in 0..3 {
        assert!(app.attempt(center.snapshot()).await.unwrap());
    }
    let calls = center.calls.lock().unwrap();
    assert_eq!(calls.len(), 6);
    for index in 0..3 {
        assert_eq!(calls[index].2, calls[index + 3].2);
        assert_eq!(calls[index].2["revision"], 1);
        assert_eq!(calls[index].2["items"], json!([]));
    }
}

#[tokio::test]
async fn ingress_uses_discovered_capabilities_keeps_offline_authority_and_refuses_old_scope() {
    let (app, center, _store, _directory) = application();
    let (sender, mut receiver) = mpsc::channel(1);
    let ingress = Ingress {
        state: center.state(),
        queue: sender,
        bytes: Arc::new(Semaphore::new(INGRESS_BYTES)),
        changes: app.sources.changes.clone(),
        native: true,
    };
    let payload = json!({
        "terminalId":"terminal","workspaceId":"workspace","title":"Done","body":"Completed"
    });
    center.state.send_modify(|state| state.methods.clear());
    assert!(!ingress.publish("terminal_attention_required", &payload));
    assert!(receiver.try_recv().is_err());
    center.state.send_modify(|state| {
        state.methods.push(EVENTS.to_owned());
        state.online = false;
    });
    assert!(ingress.publish("terminal_attention_required", &payload));
    let incoming = receiver.recv().await.unwrap();
    center
        .state
        .send_modify(|state| state.scope = Some(scope()));
    app.store_event(&incoming).await.unwrap();
    assert!(center.calls.lock().unwrap().is_empty());
    assert!(
        app.outbox
            .lock()
            .unwrap()
            .next(&incoming.scope, Utc::now().timestamp_millis())
            .is_none()
    );
}

#[tokio::test]
async fn cancellation_flushes_accepted_ingress_without_waiting_for_network() {
    let (app, center, store, _directory) = application();
    center.state.send_modify(|state| state.online = false);
    let (sender, receiver) = mpsc::channel(INGRESS_EVENTS);
    let ingress = Ingress {
        state: center.state(),
        queue: sender,
        bytes: Arc::new(Semaphore::new(INGRESS_BYTES)),
        changes: app.sources.changes.clone(),
        native: true,
    };
    assert!(ingress.publish(
        "terminal_attention_required",
        &json!({"terminalId":"t","title":"Done","body":"Completed"})
    ));
    app.cancel.cancel();
    app.clone().spool(receiver).await;
    let recovered = Outbox::open(store).unwrap();
    let job = recovered
        .next(
            center.state.borrow().scope.as_ref().unwrap(),
            Utc::now().timestamp_millis(),
        )
        .unwrap();
    assert_eq!(job.entry.params["notification"]["title"], "Done");
    assert!(center.calls.lock().unwrap().is_empty());
}

async fn wait_for(ready: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while !ready() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn background_workers_spool_offline_and_deliver_after_link_returns() {
    let (app, center, store, _directory) = application();
    center.state.send_modify(|state| state.online = false);
    let (sender, receiver) = mpsc::channel(INGRESS_EVENTS);
    let ingress = Ingress {
        state: center.state(),
        queue: sender,
        bytes: Arc::new(Semaphore::new(INGRESS_BYTES)),
        changes: app.sources.changes.clone(),
        native: true,
    };
    let spool = app.clone();
    let spool = tokio::spawn(spool.spool(receiver));
    let delivery = app.clone();
    let delivery = tokio::spawn(async move { delivery.deliver().await });
    assert!(ingress.publish(
        "terminal_attention_required",
        &json!({"terminalId":"terminal","title":"Done","body":"Completed"})
    ));
    wait_for(|| {
        store.document.lock().unwrap()["events"]
            .as_array()
            .is_some_and(|events| events.len() == 1)
    })
    .await;
    assert!(center.calls.lock().unwrap().is_empty());
    center.state.send_modify(|state| state.online = true);
    wait_for(|| center.calls.lock().unwrap().len() >= 4).await;
    assert_eq!(
        center
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, method, _)| *method == EVENTS)
            .count(),
        1
    );
    app.cancel.cancel();
    tokio::time::timeout(Duration::from_secs(3), async {
        spool.await.unwrap();
        delivery.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn migration_default_publishes_events_without_claiming_legacy_native_notifications() {
    let (mut app, center, _store, _directory) = application();
    Arc::get_mut(&mut app).unwrap().native = false;
    let (sender, mut receiver) = mpsc::channel(INGRESS_EVENTS);
    let ingress = Ingress {
        state: center.state(),
        queue: sender,
        bytes: Arc::new(Semaphore::new(INGRESS_BYTES)),
        changes: app.sources.changes.clone(),
        native: false,
    };
    assert!(!ingress.publish(
        "terminal_attention_required",
        &json!({"terminalId":"terminal","title":"Done","body":"Completed"})
    ));
    app.store_event(&receiver.recv().await.unwrap())
        .await
        .unwrap();
    let job = app
        .outbox
        .lock()
        .unwrap()
        .next(
            center.snapshot().scope.as_ref().unwrap(),
            Utc::now().timestamp_millis(),
        )
        .unwrap();
    assert!(job.entry.params.get("notification").is_none());
    assert!(app.attempt(center.snapshot()).await.unwrap());
}

#[tokio::test]
async fn attention_cleared_before_retry_is_removed_without_a_native_delivery() {
    let (app, center, _store, _directory) = application();
    let scope = center.snapshot().scope.unwrap();
    {
        let mut outbox = app.outbox.lock().unwrap();
        outbox.activate(&scope, 0).unwrap();
        outbox
            .enqueue(
                &scope,
                json!({"eventId":Uuid::new_v4(),"method":"agent_attention_required",
        "params":{"agentId":"deleted","timestamp":"now","reason":"finished"},
        "notification":{"title":"Done","body":"Completed","data":{}}}),
                Utc::now().timestamp_millis(),
            )
            .unwrap();
    }
    assert!(app.attempt(center.snapshot()).await.unwrap());
    assert!(center.calls.lock().unwrap().is_empty());
    assert!(
        app.outbox
            .lock()
            .unwrap()
            .next(&scope, Utc::now().timestamp_millis())
            .is_none()
    );
}
