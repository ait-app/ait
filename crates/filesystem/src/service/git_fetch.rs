//! Repository-wide remote refreshes owned by active workspace directory observations.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use domain::session::protocol::SessionEventKind;
use model::Runtime;
use model::session::SessionEvents;
use model::workspace::git::{WorkspaceGitObservation, WorkspaceGitObserver};
use tokio::sync::{Notify, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;

use crate::ports::git_fetch::{GitFetchError, GitFetchRuntime};

const FETCH_INTERVAL: Duration = Duration::from_secs(180);

/// Background remote refresh capability; adapters perform blocking Git work outside the reactor.
#[derive(Debug)]
pub struct GitFetch {
    backend: Arc<dyn GitFetchRuntime>,
}

#[derive(Debug, Default)]
struct Observers {
    paths: RwLock<BTreeMap<u64, BTreeSet<String>>>,
    changed: Notify,
    next_id: std::sync::atomic::AtomicU64,
}

#[derive(Debug)]
struct Source(Arc<Observers>);

#[derive(Debug)]
struct Observation {
    id: u64,
    observers: Arc<Observers>,
}

struct Worker {
    paths: Arc<RwLock<BTreeSet<String>>>,
    cancellation: CancellationToken,
    task: JoinHandle<()>,
}

struct Resources {
    backend: Arc<dyn GitFetchRuntime>,
    runtime: Arc<Runtime>,
    events: SessionEvents,
    jobs: Arc<Semaphore>,
}

impl GitFetch {
    /// Compose a background fetch service with an independently shared Git adapter.
    #[must_use]
    pub fn new(backend: Arc<dyn GitFetchRuntime>) -> Self {
        Self { backend }
    }

    /// Start tracked observation coordination using the process cancellation and event bus.
    /// No repository is fetched until a directory subscription supplies active paths.
    #[must_use]
    pub fn start(
        self,
        runtime: &Arc<Runtime>,
        events: SessionEvents,
    ) -> Arc<dyn WorkspaceGitObserver> {
        let observers = Arc::new(Observers::default());
        let source = Arc::new(Source(observers.clone()));
        let admission = runtime
            .admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !runtime.cancellation.is_cancelled() {
            runtime
                .tasks
                .spawn(coordinate(observers, self.backend, runtime.clone(), events));
        }
        drop(admission);
        source
    }
}

impl WorkspaceGitObserver for Source {
    fn observe(&self) -> Box<dyn WorkspaceGitObservation> {
        Box::new(Observation {
            id: self
                .0
                .next_id
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            observers: self.0.clone(),
        })
    }
}

impl WorkspaceGitObservation for Observation {
    fn set_paths(&mut self, paths: &[String]) {
        let next = paths.iter().cloned().collect();
        let mut observers = self
            .observers
            .paths
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if observers.get(&self.id) != Some(&next) {
            observers.insert(self.id, next);
            self.observers.changed.notify_one();
        }
    }
}

impl Drop for Observation {
    fn drop(&mut self) {
        self.observers
            .paths
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.id);
        self.observers.changed.notify_one();
    }
}

fn active_paths(observers: &Observers) -> BTreeSet<String> {
    observers
        .paths
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .values()
        .flatten()
        .cloned()
        .collect()
}

async fn coordinate(
    observers: Arc<Observers>,
    backend: Arc<dyn GitFetchRuntime>,
    runtime: Arc<Runtime>,
    events: SessionEvents,
) {
    let resources = Resources {
        backend: backend.clone(),
        runtime: runtime.clone(),
        events,
        jobs: Arc::new(Semaphore::new(2)),
    };
    let mut repositories = BTreeMap::new();
    let mut workers = BTreeMap::<PathBuf, Worker>::new();
    let mut discovery =
        tokio::time::interval_at(tokio::time::Instant::now() + FETCH_INTERVAL, FETCH_INTERVAL);
    discovery.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        let rediscover = tokio::select! {
            biased;
            () = runtime.cancellation.cancelled() => break,
            () = observers.changed.notified() => false,
            _ = discovery.tick() => true,
        };
        let paths = active_paths(&observers);
        if rediscover {
            repositories.clear();
        }
        repositories.retain(|path, _| paths.contains(path));
        let missing: Vec<_> = paths
            .difference(&repositories.keys().cloned().collect())
            .cloned()
            .collect();
        let read_backend = backend.clone();
        let cancellation = runtime.cancellation.clone();
        let resolved = if missing.is_empty() {
            BTreeMap::new()
        } else {
            tokio::task::spawn_blocking(move || {
                missing
                    .into_iter()
                    .take_while(|_| !cancellation.is_cancelled())
                    .map(|path| {
                        let repository = read_backend.repository(&path).unwrap_or_else(|error| {
                            tracing::warn!(%error, "Background Git discovery failed");
                            None
                        });
                        (path, repository)
                    })
                    .collect::<BTreeMap<_, _>>()
            })
            .await
            .unwrap_or_default()
        };
        repositories.extend(resolved);
        if runtime.cancellation.is_cancelled() {
            break;
        }
        // Recheck interest after blocking discovery; a disconnected/archived path must not start.
        let active = active_paths(&observers);
        let mut desired = BTreeMap::<PathBuf, BTreeSet<String>>::new();
        for (path, repository) in &repositories {
            if active.contains(path)
                && let Some(repository) = repository
            {
                desired
                    .entry(repository.clone())
                    .or_default()
                    .insert(path.clone());
            }
        }
        reconcile(&mut workers, desired, &resources).await;
    }
    for worker in workers.into_values() {
        worker.cancellation.cancel();
        let _ = worker.task.await;
    }
}

async fn reconcile(
    workers: &mut BTreeMap<PathBuf, Worker>,
    desired: BTreeMap<PathBuf, BTreeSet<String>>,
    resources: &Resources,
) {
    let removed: Vec<_> = workers
        .keys()
        .filter(|repository| !desired.contains_key(*repository))
        .cloned()
        .collect();
    for repository in removed {
        if let Some(worker) = workers.remove(&repository) {
            worker.cancellation.cancel();
            // Retire the previous worker before the same repository can be re-observed.
            let _ = worker.task.await;
        }
    }
    for (repository, paths) in desired {
        if let Some(worker) = workers.get_mut(&repository) {
            *worker
                .paths
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = paths;
            continue;
        }
        let admission = resources
            .runtime
            .admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if resources.runtime.cancellation.is_cancelled() {
            break;
        }
        let paths = Arc::new(RwLock::new(paths));
        let cancellation = resources.runtime.cancellation.child_token();
        let task = resources.runtime.tasks.spawn(fetch_repository(
            paths.clone(),
            resources.backend.clone(),
            cancellation.clone(),
            resources.events.clone(),
            resources.jobs.clone(),
        ));
        workers.insert(
            repository,
            Worker {
                paths,
                cancellation,
                task,
            },
        );
        drop(admission);
    }
}

async fn fetch_repository(
    paths: Arc<RwLock<BTreeSet<String>>>,
    backend: Arc<dyn GitFetchRuntime>,
    cancellation: CancellationToken,
    events: SessionEvents,
    jobs: Arc<Semaphore>,
) {
    let mut interval = tokio::time::interval(FETCH_INTERVAL);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut previous = BTreeMap::new();
    loop {
        tokio::select! {
            biased;
            () = cancellation.cancelled() => break,
            _ = interval.tick() => {},
        }
        let permit = tokio::select! {
            biased;
            () = cancellation.cancelled() => break,
            permit = jobs.clone().acquire_owned() => match permit {
                Ok(permit) => permit,
                Err(_) => break,
            },
        };
        let read_paths = paths.clone();
        let read_backend = backend.clone();
        let read_cancel = cancellation.clone();
        let snapshots = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            fetch_snapshots(&read_paths, read_backend.as_ref(), &read_cancel)
        })
        .await;
        if cancellation.is_cancelled() {
            break;
        }
        if let Ok(snapshots) = snapshots {
            previous.retain(|path, _| snapshots.contains_key(path));
            for (path, snapshot) in snapshots {
                if previous.get(&path) != Some(&snapshot) {
                    events.publish(SessionEventKind::CheckoutStatus, &snapshot);
                    previous.insert(path, snapshot);
                }
            }
        }
    }
}

fn fetch_snapshots(
    paths: &RwLock<BTreeSet<String>>,
    backend: &dyn GitFetchRuntime,
    cancellation: &CancellationToken,
) -> BTreeMap<String, serde_json::Value> {
    let cwd = paths
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .first()
        .cloned();
    let Some(cwd) = cwd else {
        return BTreeMap::new();
    };
    if let Err(error) = backend.fetch(&cwd, cancellation) {
        if error == GitFetchError::Cancelled {
            return BTreeMap::new();
        }
        tracing::warn!(%error, "Background Git fetch failed; retrying on the next interval");
    }
    let observed = paths
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    observed
        .into_iter()
        .take_while(|_| !cancellation.is_cancelled())
        .filter_map(|path| {
            let status = backend.status(&path).ok()?;
            let payload = crate::rpc::checkout::protocol_status(&path, Ok(status));
            serde_json::to_value(payload)
                .ok()
                .map(|payload| (path, payload))
        })
        .collect()
}

#[cfg(test)]
mod tests;
