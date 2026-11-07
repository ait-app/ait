//! Bounded, demand-driven cache of Workspace Git and Forge presentation facts.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use domain::workspace::runtime::{WorkspaceForgeSnapshot, WorkspaceRuntimeSnapshot};
use model::workspace::runtime::WorkspaceRuntimeSource;

use crate::forge::local::forge::LocalForge;
use crate::git::local::checkout::LocalCheckout;

mod forge;
mod reads;
mod watch;

const CACHE_CAPACITY: usize = 1024;
const READ_CONCURRENCY: usize = 2;
/// Unwatched local checkouts refresh quickly while they change and back off while idle.
const GIT_REFRESH: Refresh = Refresh {
    base: Duration::from_secs(2),
    max: Duration::from_secs(30),
};
/// Watched checkouts refresh after file events; polling only guards against missed events.
const WATCHED_GIT_REFRESH: Refresh = Refresh {
    base: GIT_REFRESH.base,
    max: Duration::from_secs(300),
};
/// Forge reads use the network, so idle pull requests are polled least often.
const FORGE_REFRESH: Refresh = Refresh {
    base: Duration::from_secs(30),
    max: Duration::from_secs(120),
};

/// Checkout facts shared across directory clones and physical client connections.
///
/// Reads return immediately. Only directories requested by a live list/update reader refresh;
/// Git and Forge have separate admission limits, so a slow remote cannot block local facts.
#[derive(Debug, Clone)]
pub struct LocalWorkspaceRuntime {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    checkout: LocalCheckout,
    forge: LocalForge,
    cache: Mutex<BTreeMap<PathBuf, Entry>>,
    git_reads: Arc<AtomicUsize>,
    forge_reads: Arc<AtomicUsize>,
    git_jobs: Mutex<VecDeque<reads::Job>>,
    forge_jobs: Mutex<VecDeque<reads::Job>>,
    git_refresh: Refresh,
    watched_git_refresh: Refresh,
    forge_refresh: Refresh,
    changes: Option<model::changes::Changes>,
    #[cfg(target_os = "macos")]
    watch: watch::Watch,
}

impl Inner {
    /// Git refresh policy for `entry`, relaxed once file events cover it.
    fn git_refresh(&self, entry: &Entry) -> Refresh {
        if entry.watched {
            self.watched_git_refresh
        } else {
            self.git_refresh
        }
    }
}

/// Exponential refresh backoff: each read that leaves the cached value unchanged doubles the
/// interval from `base` up to `max`, and any change returns it to `base`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Refresh {
    base: Duration,
    max: Duration,
}

impl Refresh {
    fn interval(self, unchanged: u32) -> Duration {
        self.base
            .saturating_mul(2_u32.saturating_pow(unchanged))
            .min(self.max)
    }
}

#[derive(Debug)]
struct Entry {
    value: WorkspaceRuntimeSnapshot,
    head: Option<String>,
    git_read: ReadState,
    forge_read: ReadState,
    accessed_at: Instant,
    /// Watched paths learned from the latest Git read.
    roots: Option<watch::WatchRoots>,
    /// File events currently cover every root.
    watched: bool,
}

#[derive(Debug, Default)]
struct ReadState {
    completed_at: Option<Instant>,
    running: bool,
    /// Consecutive completed reads that left the cached value unchanged.
    unchanged: u32,
    /// A file event arrived since the latest read started.
    dirty: bool,
}

impl ReadState {
    /// Whether a read should start: never read, idle past its backoff, or dirty past `base`.
    fn due(&self, refresh: Refresh) -> bool {
        !self.running
            && self.completed_at.is_none_or(|last| {
                let elapsed = last.elapsed();
                elapsed >= refresh.interval(self.unchanged)
                    || (self.dirty && elapsed >= refresh.base)
            })
    }

    /// Admit a read; events arriving while it runs mark the state dirty again.
    fn start(&mut self) {
        self.running = true;
        self.dirty = false;
    }

    /// Release an admitted read that never ran, keeping it due for the next reader.
    fn abandon(&mut self) {
        self.running = false;
        self.dirty = true;
    }

    /// Record a completed read; `changed` resets the backoff, otherwise it grows.
    fn finish(&mut self, changed: bool) {
        self.running = false;
        self.completed_at = Some(Instant::now());
        self.unchanged = if changed {
            0
        } else {
            self.unchanged.saturating_add(1)
        };
    }
}

#[derive(Debug)]
struct Permit(Arc<AtomicUsize>);

impl Permit {
    fn acquire(reads: &Arc<AtomicUsize>) -> Option<Self> {
        reads
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                (count < READ_CONCURRENCY).then_some(count + 1)
            })
            .ok()
            .map(|_| Self(reads.clone()))
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

impl LocalWorkspaceRuntime {
    /// Compose cached observations using the same local adapters as explicit Git/PR requests.
    ///
    /// `checkout` supplies bounded local reads; `forge` reuses the host's existing CLI auth.
    /// No commands start until a Workspace directory snapshot requests a checkout.
    #[must_use]
    pub fn new(checkout: LocalCheckout, forge: LocalForge) -> Self {
        Self {
            inner: Arc::new(Inner {
                checkout,
                forge,
                cache: Mutex::default(),
                git_reads: Arc::default(),
                forge_reads: Arc::default(),
                git_jobs: Mutex::default(),
                forge_jobs: Mutex::default(),
                git_refresh: GIT_REFRESH,
                watched_git_refresh: WATCHED_GIT_REFRESH,
                forge_refresh: FORGE_REFRESH,
                changes: None,
                #[cfg(target_os = "macos")]
                watch: watch::Watch::default(),
            }),
        }
    }

    /// Wake Workspace directory observers when a background read completes.
    ///
    /// # Panics
    /// Panics if the runtime has already been cloned before composition completes.
    #[must_use]
    pub fn with_changes(mut self, changes: model::changes::Changes) -> Self {
        Arc::get_mut(&mut self.inner)
            .expect("new Workspace runtime has one owner")
            .changes = Some(changes);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Identity {
    head: Option<String>,
    branch: Option<String>,
    remote: Option<String>,
}

impl Identity {
    fn from_entry(entry: &Entry) -> Self {
        Self {
            head: entry.head.clone(),
            branch: entry
                .value
                .git
                .as_ref()
                .and_then(|git| git.current_branch.clone()),
            remote: entry
                .value
                .git
                .as_ref()
                .and_then(|git| git.remote_url.clone()),
        }
    }
}

impl WorkspaceRuntimeSource for LocalWorkspaceRuntime {
    fn snapshot(&self, cwd: &str) -> WorkspaceRuntimeSnapshot {
        let cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| PathBuf::from(cwd));
        let mut cache = self
            .inner
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut evicted = false;
        if !cache.contains_key(&cwd) && cache.len() >= CACHE_CAPACITY {
            let oldest = cache
                .iter()
                .filter(|(_, entry)| !entry.git_read.running && !entry.forge_read.running)
                .min_by_key(|(_, entry)| entry.accessed_at)
                .map(|(cwd, _)| cwd.clone());
            if let Some(oldest) = oldest {
                evicted = cache
                    .remove(&oldest)
                    .is_some_and(|entry| entry.roots.is_some());
            } else {
                return WorkspaceRuntimeSnapshot::default();
            }
        }
        let entry = cache.entry(cwd.clone()).or_insert_with(|| Entry {
            value: WorkspaceRuntimeSnapshot::default(),
            head: None,
            git_read: ReadState::default(),
            forge_read: ReadState::default(),
            accessed_at: Instant::now(),
            roots: None,
            watched: false,
        });
        entry.accessed_at = Instant::now();
        let git_due = entry.git_read.due(self.inner.git_refresh(entry));
        if git_due {
            entry.git_read.start();
        }
        let forge_due = if entry
            .value
            .git
            .as_ref()
            .is_some_and(|git| git.remote_url.is_some())
        {
            entry.forge_read.due(self.inner.forge_refresh)
        } else {
            if entry.git_read.completed_at.is_some() {
                entry.value.forge = Some(WorkspaceForgeSnapshot::default());
            }
            false
        };
        if forge_due {
            entry.forge_read.running = true;
        }
        let identity = Identity::from_entry(entry);
        let snapshot = entry.value.clone();
        drop(cache);
        if evicted {
            watch::sync(&self.inner);
        }
        if git_due {
            reads::enqueue(&self.inner, reads::Job::Git(cwd.clone()));
        }
        if forge_due {
            reads::enqueue(&self.inner, reads::Job::Forge(cwd, identity));
        }
        snapshot
    }
}

#[cfg(test)]
mod tests;
