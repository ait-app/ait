use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use domain::agent_runtime::PersistedAgentRuntimeRecord;
use domain::agent_runtime::registry::AgentRuntimeRegistryError;
use domain::session::protocol::EventsRequest;
use model::session::SessionEvents;
use model::{Lifecycle, Limits, Runtime, ServerInfo, VERSION};
use serde_json::json;

use super::*;
use crate::git::ports::checkout::{CheckoutRuntimeError, CheckoutStatus};
use crate::git::ports::git_fetch::GitFetchError;
use crate::git::service::git_fetch::GitFetch;

#[test]
fn reads_at_once_then_collapses_requests_until_the_interval_elapses() {
    let start = Instant::now();
    let mut throttle = Throttle::default();
    throttle.request("a".to_owned(), start);
    assert_eq!(throttle.take_due(start), vec!["a".to_owned()]);
    let later = start + Duration::from_secs(10);
    throttle.request("a".to_owned(), later);
    throttle.request("a".to_owned(), later + Duration::from_secs(5));
    assert_eq!(throttle.next_due(), Some(start + MIN_INTERVAL));
    assert!(
        throttle
            .take_due(start + Duration::from_secs(59))
            .is_empty()
    );
    assert_eq!(
        throttle.take_due(start + MIN_INTERVAL),
        vec!["a".to_owned()]
    );
    assert_eq!(throttle.next_due(), None);
}

#[test]
fn throttles_each_checkout_independently_and_forgets_old_reads() {
    let start = Instant::now();
    let mut throttle = Throttle::default();
    throttle.request("a".to_owned(), start);
    assert_eq!(throttle.take_due(start), vec!["a".to_owned()]);
    throttle.request("b".to_owned(), start);
    assert_eq!(throttle.take_due(start), vec!["b".to_owned()]);
    let after = start + MIN_INTERVAL;
    assert!(throttle.take_due(after).is_empty());
    assert!(throttle.last_read.is_empty());
    throttle.request("a".to_owned(), after);
    assert_eq!(throttle.take_due(after), vec!["a".to_owned()]);
}

#[derive(Debug, Default)]
struct Registry(Mutex<BTreeMap<String, PersistedAgentRuntimeRecord>>);

impl Registry {
    fn add(&self, id: &str, cwd: &str) {
        let record = serde_json::from_value(json!({
            "id": id, "provider": "test", "cwd": cwd,
            "createdAt": "now", "updatedAt": "now",
        }))
        .unwrap();
        self.0.lock().unwrap().insert(id.to_owned(), record);
    }
}

impl AgentRuntimeRegistry for Registry {
    fn initialize(&self) -> Result<(), AgentRuntimeRegistryError> {
        Ok(())
    }
    fn list(&self) -> Result<Vec<PersistedAgentRuntimeRecord>, AgentRuntimeRegistryError> {
        Ok(self.0.lock().unwrap().values().cloned().collect())
    }
    fn get(
        &self,
        id: &str,
    ) -> Result<Option<PersistedAgentRuntimeRecord>, AgentRuntimeRegistryError> {
        Ok(self.0.lock().unwrap().get(id).cloned())
    }
    fn upsert(
        &self,
        record: &PersistedAgentRuntimeRecord,
    ) -> Result<(), AgentRuntimeRegistryError> {
        self.0
            .lock()
            .unwrap()
            .insert(record.id.clone(), record.clone());
        Ok(())
    }
    fn update(
        &self,
        _: &str,
        _: &dyn Fn(&PersistedAgentRuntimeRecord) -> PersistedAgentRuntimeRecord,
    ) -> Result<Option<PersistedAgentRuntimeRecord>, AgentRuntimeRegistryError> {
        unreachable!("turn ends only read Agents")
    }
    fn remove(&self, id: &str) -> Result<bool, AgentRuntimeRegistryError> {
        Ok(self.0.lock().unwrap().remove(id).is_some())
    }
}

/// A Git adapter whose only observable state is whether the checkout is dirty.
#[derive(Debug, Default)]
struct Backend {
    dirty: AtomicBool,
    reads: AtomicUsize,
}

impl GitFetchRuntime for Backend {
    fn repository(&self, _: &str) -> Result<Option<PathBuf>, GitFetchError> {
        Ok(None)
    }
    fn fetch(&self, _: &str, _: &CancellationToken) -> Result<(), GitFetchError> {
        unreachable!("no repository is observed")
    }
    fn status(&self, cwd: &str) -> Result<CheckoutStatus, CheckoutRuntimeError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(CheckoutStatus {
            is_git: true,
            repo_root: Some(cwd.to_owned()),
            main_repo_root: None,
            current_branch: Some("feature".to_owned()),
            is_dirty: Some(self.dirty.load(Ordering::SeqCst)),
            branch_status: None,
            base_ref: Some("main".to_owned()),
            ahead_behind: None,
            upstream_ref: None,
            ahead_of_origin: None,
            behind_of_origin: None,
            has_remote: false,
            remote_url: None,
            is_managed_worktree: false,
        })
    }
}

struct Harness {
    runtime: Arc<Runtime>,
    events: SessionEvents,
    backend: Arc<Backend>,
    updates: Arc<Mutex<Vec<Value>>>,
    _connection: model::session::SessionConnection,
    _subscription: model::session::SessionSubscription,
}

impl Harness {
    fn new(registry: Registry) -> Self {
        let runtime = Arc::new(Runtime::new(ServerInfo {
            server_id: "server".to_owned(),
            instance_id: "instance".to_owned(),
            version: None,
            listen: "127.0.0.1:1".to_owned(),
            lifecycle: Lifecycle::Ready,
            protocol: VERSION,
            capabilities: Vec::new(),
            implemented_capabilities: Vec::new(),
            features: Vec::new(),
            limits: Limits::default(),
        }));
        let events = SessionEvents::default();
        let connection = events.connect();
        let updates = Arc::new(Mutex::new(Vec::new()));
        let captured = updates.clone();
        let subscription = connection
            .subscribe(
                EventsRequest {
                    events: vec!["checkout_status_update".to_owned()],
                    notifications: false,
                },
                Arc::new(move |_, payload| {
                    captured.lock().unwrap().push(payload);
                    Ok(())
                }),
            )
            .unwrap();
        subscription.activate().unwrap();
        let backend = Arc::new(Backend::default());
        let _source = GitFetch::new(backend.clone())
            .with_agent_turns(Arc::new(registry))
            .start(&runtime, events.clone());
        Self {
            runtime,
            events,
            backend,
            updates,
            _connection: connection,
            _subscription: subscription,
        }
    }

    fn turn_end(&self, agent: &str, reason: &str) {
        self.events.publish(
            SessionEventKind::AgentAttention,
            &json!({"agentId": agent, "reason": reason, "timestamp": "now"}),
        );
    }

    fn reads(&self) -> usize {
        self.backend.reads.load(Ordering::SeqCst)
    }

    fn updates(&self) -> Vec<Value> {
        self.updates.lock().unwrap().clone()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.runtime.cancellation.cancel();
    }
}

// Keep a runnable task while Tokio time is paused so blocking Git mocks cannot auto-advance it.
async fn wait_for(condition: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            std::time::Instant::now() < deadline,
            "background operation did not complete"
        );
        tokio::task::yield_now().await;
    }
}

/// Let the background task drain every queued event without advancing paused time.
async fn settle() {
    for _ in 0..100 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test(start_paused = true)]
async fn publishes_the_checkout_status_when_a_turn_ends() {
    let registry = Registry::default();
    registry.add("agent", "/work/feature");
    let harness = Harness::new(registry);
    harness.backend.dirty.store(true, Ordering::SeqCst);
    harness.turn_end("agent", "finished");
    wait_for(|| harness.updates().len() == 1).await;
    let update = &harness.updates()[0];
    assert_eq!(update["cwd"], "/work/feature");
    assert_eq!(update["isDirty"], true);
    harness.runtime.cancellation.cancel();
    harness.runtime.tasks.close();
    wait_for(|| harness.runtime.tasks.is_empty()).await;
}

#[tokio::test(start_paused = true)]
async fn ignores_permission_requests_and_unknown_agents() {
    let registry = Registry::default();
    registry.add("agent", "/work/feature");
    let harness = Harness::new(registry);
    harness.turn_end("agent", "permission");
    harness.turn_end("missing", "finished");
    settle().await;
    assert_eq!(harness.reads(), 0);
    assert!(harness.updates().is_empty());
}

#[tokio::test(start_paused = true)]
async fn reads_a_checkout_at_most_once_a_minute_and_catches_up_after() {
    let registry = Registry::default();
    registry.add("first", "/work/feature");
    registry.add("second", "/work/feature");
    let harness = Harness::new(registry);
    harness.turn_end("first", "finished");
    wait_for(|| harness.updates().len() == 1).await;
    harness.backend.dirty.store(true, Ordering::SeqCst);
    tokio::time::advance(Duration::from_secs(10)).await;
    harness.turn_end("second", "error");
    harness.turn_end("first", "finished");
    settle().await;
    assert_eq!(harness.reads(), 1);
    tokio::time::advance(Duration::from_secs(49)).await;
    settle().await;
    assert_eq!(harness.reads(), 1);
    tokio::time::advance(Duration::from_secs(1)).await;
    wait_for(|| harness.updates().len() == 2).await;
    assert_eq!(harness.reads(), 2);
    assert_eq!(harness.updates()[1]["isDirty"], true);
}

#[tokio::test(start_paused = true)]
async fn does_not_republish_an_unchanged_status() {
    let registry = Registry::default();
    registry.add("agent", "/work/feature");
    let harness = Harness::new(registry);
    harness.turn_end("agent", "finished");
    wait_for(|| harness.updates().len() == 1).await;
    tokio::time::advance(MIN_INTERVAL).await;
    harness.turn_end("agent", "finished");
    wait_for(|| harness.reads() == 2).await;
    // Reads run one after another, so the third update proves the second read published none.
    harness.backend.dirty.store(true, Ordering::SeqCst);
    tokio::time::advance(MIN_INTERVAL).await;
    harness.turn_end("agent", "finished");
    wait_for(|| harness.reads() == 3 && harness.updates().len() == 2).await;
    assert_eq!(harness.updates()[1]["isDirty"], true);
}
