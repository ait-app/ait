use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use model::session::protocol::EventsRequest;
use model::{Lifecycle, Limits, ServerInfo, VERSION};
use serde_json::Value;

use super::*;
use crate::ports::checkout::{CheckoutRuntimeError, CheckoutStatus};

#[derive(Debug, Default)]
struct Backend {
    repositories: RwLock<BTreeMap<String, PathBuf>>,
    calls: Mutex<Vec<String>>,
    blocked: AtomicBool,
    failures: AtomicUsize,
    cancelled: AtomicUsize,
    active: AtomicUsize,
    maximum: AtomicUsize,
    behind: AtomicU64,
}

impl GitFetchRuntime for Backend {
    fn repository(&self, cwd: &str) -> Result<Option<PathBuf>, GitFetchError> {
        Ok(self.repositories.read().unwrap().get(cwd).cloned())
    }

    fn fetch(&self, cwd: &str, cancellation: &CancellationToken) -> Result<(), GitFetchError> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.maximum.fetch_max(active, Ordering::SeqCst);
        self.calls.lock().unwrap().push(cwd.to_owned());
        while self.blocked.load(Ordering::SeqCst) && !cancellation.is_cancelled() {
            std::thread::sleep(Duration::from_millis(1));
        }
        self.active.fetch_sub(1, Ordering::SeqCst);
        if cancellation.is_cancelled() {
            self.cancelled.fetch_add(1, Ordering::SeqCst);
            return Err(GitFetchError::Cancelled);
        }
        if self
            .failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            return Err(GitFetchError::Failed);
        }
        Ok(())
    }

    fn status(&self, cwd: &str) -> Result<CheckoutStatus, CheckoutRuntimeError> {
        Ok(CheckoutStatus {
            is_git: true,
            repo_root: Some(cwd.to_owned()),
            main_repo_root: None,
            current_branch: Some("feature".to_owned()),
            is_dirty: Some(false),
            branch_status: None,
            base_ref: Some("main".to_owned()),
            ahead_behind: None,
            upstream_ref: Some("origin/feature".to_owned()),
            ahead_of_origin: Some(0),
            behind_of_origin: Some(self.behind.load(Ordering::SeqCst)),
            has_remote: true,
            remote_url: Some("https://example.test/repo".to_owned()),
            is_managed_worktree: false,
        })
    }
}

struct Harness {
    runtime: Arc<Runtime>,
    backend: Arc<Backend>,
    source: Arc<dyn WorkspaceGitObserver>,
    updates: Arc<Mutex<Vec<Value>>>,
    _connection: model::session::SessionConnection,
    _subscription: model::session::SessionSubscription,
}

impl Harness {
    fn new() -> Self {
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
                Arc::new(move |kind, payload| {
                    assert_eq!(kind.method(), "checkout.status.update");
                    captured.lock().unwrap().push(payload);
                    Ok(())
                }),
            )
            .unwrap();
        subscription.activate().unwrap();
        let backend = Arc::new(Backend::default());
        let source = GitFetch::new(backend.clone()).start(&runtime, events);
        Self {
            runtime,
            backend,
            source,
            updates,
            _connection: connection,
            _subscription: subscription,
        }
    }

    fn observe(&self, paths: &[&str]) -> Box<dyn WorkspaceGitObservation> {
        let mut observation = self.source.observe();
        observation.set_paths(
            &paths
                .iter()
                .map(|path| (*path).to_owned())
                .collect::<Vec<_>>(),
        );
        observation
    }

    fn add(&self, path: &str, repository: &str) {
        self.backend
            .repositories
            .write()
            .unwrap()
            .insert(path.to_owned(), repository.into());
    }

    fn calls(&self) -> usize {
        self.backend.calls.lock().unwrap().len()
    }

    async fn stop(&self) {
        self.runtime.cancellation.cancel();
        self.runtime.tasks.close();
        wait_for(|| self.runtime.tasks.is_empty()).await;
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.runtime.cancellation.cancel();
    }
}

// Keep a runnable task while Tokio time is paused so blocking Git mocks cannot auto-advance it.
async fn wait_for(condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "background operation did not complete"
        );
        tokio::task::yield_now().await;
    }
}

#[tokio::test(start_paused = true)]
async fn fetches_immediately_deduplicates_worktrees_and_publishes_each_status() {
    let harness = Harness::new();
    harness.add("main", "shared");
    harness.add("linked", "shared");
    let first = harness.observe(&["main", "linked", "non-git", "no-origin"]);
    let second = harness.observe(&["linked"]);
    wait_for(|| harness.updates.lock().unwrap().len() == 2).await;
    assert_eq!(harness.calls(), 1);
    assert_eq!(harness.runtime.jobs.available_permits(), 1);
    let paths: BTreeSet<_> = harness
        .updates
        .lock()
        .unwrap()
        .iter()
        .map(|update| update["cwd"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        paths,
        BTreeSet::from(["main".to_owned(), "linked".to_owned()])
    );
    drop(first);
    tokio::time::advance(Duration::from_secs(180)).await;
    wait_for(|| harness.calls() == 2).await;
    drop(second);
    harness.stop().await;
}

#[tokio::test(start_paused = true)]
async fn repeats_every_three_minutes_and_suppresses_unchanged_events() {
    let harness = Harness::new();
    harness.add("feature", "shared");
    let _observation = harness.observe(&["feature"]);
    wait_for(|| harness.updates.lock().unwrap().len() == 1).await;
    tokio::time::advance(Duration::from_secs(179)).await;
    assert_eq!(harness.calls(), 1);
    tokio::time::advance(Duration::from_secs(1)).await;
    wait_for(|| harness.calls() == 2 && harness.backend.active.load(Ordering::SeqCst) == 0).await;
    assert_eq!(harness.updates.lock().unwrap().len(), 1);
    harness.backend.behind.store(2, Ordering::SeqCst);
    tokio::time::advance(FETCH_INTERVAL).await;
    wait_for(|| harness.updates.lock().unwrap().len() == 2).await;
    assert_eq!(harness.updates.lock().unwrap()[1]["behindOfOrigin"], 2);
    harness.stop().await;
}

#[tokio::test(start_paused = true)]
async fn release_cancels_in_flight_fetch_and_reobservation_never_overlaps() {
    let harness = Harness::new();
    harness.add("feature", "shared");
    harness.backend.blocked.store(true, Ordering::SeqCst);
    let first = harness.observe(&["feature"]);
    let mut second = harness.observe(&["feature"]);
    wait_for(|| harness.calls() == 1).await;
    drop(first);
    second.set_paths(&[]);
    wait_for(|| harness.backend.cancelled.load(Ordering::SeqCst) == 1).await;
    assert!(harness.updates.lock().unwrap().is_empty());
    harness.backend.blocked.store(false, Ordering::SeqCst);
    second.set_paths(&["feature".to_owned()]);
    wait_for(|| harness.updates.lock().unwrap().len() == 1).await;
    assert_eq!(harness.calls(), 2);
    assert_eq!(harness.backend.maximum.load(Ordering::SeqCst), 1);
    drop(second);
    tokio::time::advance(FETCH_INTERVAL).await;
    harness.stop().await;
    assert_eq!(harness.calls(), 2);
}

#[tokio::test(start_paused = true)]
async fn shutdown_cancels_fetch_and_drains_the_blocking_worker() {
    let harness = Harness::new();
    harness.add("feature", "shared");
    harness.backend.blocked.store(true, Ordering::SeqCst);
    let _observation = harness.observe(&["feature"]);
    wait_for(|| harness.calls() == 1).await;
    harness.stop().await;
    assert_eq!(harness.backend.cancelled.load(Ordering::SeqCst), 1);
    assert_eq!(harness.backend.active.load(Ordering::SeqCst), 0);
    assert!(harness.updates.lock().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn failed_fetch_retries_and_new_origins_are_discovered() {
    let harness = Harness::new();
    let _observation = harness.observe(&["feature"]);
    // An unconfigured directory is not an eligible fetch target.
    tokio::task::yield_now().await;
    assert_eq!(harness.calls(), 0);
    harness.add("feature", "shared");
    harness.backend.failures.store(1, Ordering::SeqCst);
    tokio::time::advance(FETCH_INTERVAL).await;
    wait_for(|| harness.updates.lock().unwrap().len() == 1).await;
    assert_eq!(harness.calls(), 1);
    assert_eq!(harness.backend.failures.load(Ordering::SeqCst), 0);
    harness.backend.behind.store(1, Ordering::SeqCst);
    tokio::time::advance(FETCH_INTERVAL).await;
    wait_for(|| harness.updates.lock().unwrap().len() == 2).await;
    assert_eq!(harness.calls(), 2);
    harness.stop().await;
}

#[tokio::test(start_paused = true)]
async fn limits_concurrency_across_repositories_without_using_foreground_permits() {
    let harness = Harness::new();
    for path in ["one", "two", "three"] {
        harness.add(path, path);
    }
    harness.backend.blocked.store(true, Ordering::SeqCst);
    let _observation = harness.observe(&["one", "two", "three"]);
    wait_for(|| harness.calls() == 2).await;
    assert_eq!(harness.runtime.jobs.available_permits(), 1);
    assert_eq!(harness.backend.maximum.load(Ordering::SeqCst), 2);
    harness.stop().await;
    assert_eq!(harness.calls(), 2);
}

#[tokio::test(start_paused = true)]
async fn repeated_fetch_failures_keep_runtime_ready_and_foreground_jobs_available() {
    let harness = Harness::new();
    harness.add("feature", "shared");
    harness.backend.failures.store(3, Ordering::SeqCst);
    let _observation = harness.observe(&["feature"]);
    for attempt in 1..=3 {
        if attempt > 1 {
            tokio::time::advance(FETCH_INTERVAL).await;
        }
        wait_for(|| {
            harness.calls() == attempt
                && harness.backend.failures.load(Ordering::SeqCst) == 3 - attempt
        })
        .await;
        assert_eq!(harness.runtime.info().lifecycle, Lifecycle::Ready);
        assert!(!harness.runtime.cancellation.is_cancelled());
        assert_eq!(
            harness
                .runtime
                .run(
                    Some(Arc::new(Mutex::new(()))),
                    model::ErrorCode::RegistryIo,
                    |()| Ok(())
                )
                .await,
            Ok(())
        );
    }
    harness.backend.behind.store(1, Ordering::SeqCst);
    tokio::time::advance(FETCH_INTERVAL).await;
    wait_for(|| {
        harness.calls() == 4
            && harness
                .updates
                .lock()
                .unwrap()
                .last()
                .is_some_and(|update| update["behindOfOrigin"] == 1)
    })
    .await;
    harness.stop().await;
}
