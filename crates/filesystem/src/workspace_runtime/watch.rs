//! File-system invalidation of cached checkout facts.
//!
//! On macOS one `FSEvents` stream covers every cached work tree and Git directory. A relevant
//! change marks the checkout dirty and admits a read when one is due, so idle checkouts run no
//! Git commands; the polling backoff remains as a safety net for missed events. Other platforms
//! keep polling only, avoiding one `inotify` watch per directory in large trees.

#[cfg(any(target_os = "macos", test))]
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::Inner;
use crate::git::local::checkout::summary::GitDirs;

/// Paths whose changes can alter one cached checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WatchRoots {
    work_tree: PathBuf,
    git: Option<GitDirs>,
}

impl WatchRoots {
    /// Roots for a completed read of `cwd`; non-Git directories watch `cwd` itself.
    pub(super) fn new(cwd: &Path, git: Option<GitDirs>) -> Self {
        let work_tree = git
            .as_ref()
            .map_or_else(|| cwd.to_path_buf(), |git| git.work_tree.clone());
        Self { work_tree, git }
    }

    #[cfg(any(target_os = "macos", test))]
    fn paths(&self) -> impl Iterator<Item = &Path> {
        std::iter::once(self.work_tree.as_path()).chain(
            self.git
                .iter()
                .flat_map(|git| [git.git_dir.as_path(), git.common_dir.as_path()]),
        )
    }

    /// Whether recursive watches on `watched` roots deliver every change to this checkout.
    #[cfg(any(target_os = "macos", test))]
    fn covered_by(&self, watched: &BTreeSet<PathBuf>) -> bool {
        self.paths()
            .all(|path| watched.iter().any(|root| path.starts_with(root)))
    }

    /// Whether a change at canonical `path` can alter this checkout's facts.
    ///
    /// Object and reflog writes never do, nor do other worktrees' private state; a linked
    /// worktree shares only references, configuration and exclude rules with its common
    /// directory.
    #[cfg(any(target_os = "macos", test))]
    fn affects(&self, path: &Path) -> bool {
        let path = path.as_os_str().as_encoded_bytes();
        if let Some(git) = &self.git {
            let git_dir = git.git_dir.as_os_str().as_encoded_bytes();
            let common_dir = git.common_dir.as_os_str().as_encoded_bytes();
            if let Some(rest) = below(path, git_dir) {
                let component = first_component(rest);
                return !matches!(component, b"objects" | b"logs")
                    && (git_dir != common_dir || component != b"worktrees");
            }
            if let Some(rest) = below(path, common_dir) {
                return matches!(
                    first_component(rest),
                    b"refs" | b"packed-refs" | b"config" | b"reftable" | b"info"
                );
            }
        }
        below(path, self.work_tree.as_os_str().as_encoded_bytes()).is_some()
    }
}

/// Remainder of `path` below `root`, compared as bytes so per-event checks stay cheap.
#[cfg(any(target_os = "macos", test))]
fn below<'a>(path: &'a [u8], root: &[u8]) -> Option<&'a [u8]> {
    let rest = path.strip_prefix(root)?;
    match rest.split_first() {
        None => Some(rest),
        Some((b'/', rest)) => Some(rest),
        Some(_) => None,
    }
}

#[cfg(any(target_os = "macos", test))]
fn first_component(rest: &[u8]) -> &[u8] {
    rest.split(|byte| *byte == b'/').next().unwrap_or_default()
}

/// Smallest set of roots whose recursive watches cover every path in `paths`.
#[cfg(any(target_os = "macos", test))]
fn cover<'a>(paths: impl Iterator<Item = &'a Path>) -> BTreeSet<PathBuf> {
    let sorted: BTreeSet<&Path> = paths.collect();
    let mut covered: BTreeSet<PathBuf> = BTreeSet::new();
    for path in sorted {
        // Component order places each path's descendants directly after it.
        if covered.last().is_none_or(|root| !path.starts_with(root)) {
            covered.insert(path.to_path_buf());
        }
    }
    covered
}

/// Lazily started watcher shared by every clone of the runtime.
#[cfg(target_os = "macos")]
#[derive(Debug, Default)]
pub(super) struct Watch {
    state: std::sync::Mutex<native::State>,
}

/// Watch exactly the roots of the cached checkouts, starting the watcher on first use, and
/// record on each entry whether file events cover it.
///
/// Must be called without holding the cache lock: event delivery takes that lock, and changing
/// the watched set waits for delivery to pause.
pub(super) fn sync(inner: &Arc<Inner>) {
    #[cfg(target_os = "macos")]
    native::sync(inner);
    #[cfg(not(target_os = "macos"))]
    let _ = inner;
}

#[cfg(target_os = "macos")]
mod native {
    use std::collections::BTreeSet;
    use std::path::PathBuf;
    use std::sync::{Arc, Weak};

    use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

    use super::super::reads::{self, DEMAND_TTL, Job};
    use super::super::{Entry, Inner};
    use super::{WatchRoots, cover};

    #[derive(Debug, Default)]
    pub(super) struct State {
        watcher: Option<RecommendedWatcher>,
        watched: BTreeSet<PathBuf>,
        unavailable: bool,
    }

    impl Drop for State {
        /// Stopping a stream waits for its run loop to idle, which never happens when the last
        /// runtime handle is released by an event callback on that run loop; stop elsewhere.
        fn drop(&mut self) {
            if let Some(watcher) = self.watcher.take() {
                let _ = std::thread::Builder::new()
                    .name("workspace-watch-stop".to_owned())
                    .spawn(move || drop(watcher));
            }
        }
    }

    /// Serialized by the state lock, so concurrent callers never apply a stale watched set.
    /// Lock order is state, then cache; event delivery takes only the cache lock.
    pub(super) fn sync(inner: &Arc<Inner>) {
        let mut state = inner
            .watch
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let State {
            watcher,
            watched,
            unavailable,
        } = &mut *state;
        if watcher.is_none() && !*unavailable {
            let events = Arc::downgrade(inner);
            match notify::recommended_watcher(move |event| observe(&events, &event)) {
                Ok(started) => *watcher = Some(started),
                Err(error) => {
                    tracing::warn!(%error, "Workspace checkout watcher unavailable; polling");
                    *unavailable = true;
                }
            }
        }
        let Some(watcher) = watcher.as_mut() else {
            return;
        };
        let desired = cover(
            lock_cache(inner)
                .values()
                .filter_map(|entry| entry.roots.as_ref())
                .flat_map(WatchRoots::paths),
        );
        // A missing root cannot be watched; skipping it avoids restarting the stream for it.
        let added: Vec<PathBuf> = desired
            .difference(watched)
            .filter(|path| path.exists())
            .cloned()
            .collect();
        let removed: Vec<PathBuf> = watched.difference(&desired).cloned().collect();
        let restarted = !added.is_empty() || !removed.is_empty();
        if restarted {
            // One batch restarts the stream once; the cache lock is not held while it pauses.
            let mut paths = watcher.paths_mut();
            for path in &removed {
                let _ = paths.remove(path);
                watched.remove(path);
            }
            for path in added {
                match paths.add(&path, RecursiveMode::Recursive) {
                    Ok(()) => {
                        watched.insert(path);
                    }
                    Err(error) => tracing::debug!(%error, path = %path.display(), "watch failed"),
                }
            }
            if let Err(error) = paths.commit() {
                tracing::warn!(%error, "Workspace checkout watcher stopped; polling");
                watched.clear();
            }
        }
        for entry in lock_cache(inner).values_mut() {
            entry.watched = entry
                .roots
                .as_ref()
                .is_some_and(|roots| roots.covered_by(watched));
            // Changes during a restart, or before a checkout's first watch, produced no event.
            if restarted && entry.watched {
                entry.git_read.dirty = true;
            }
        }
    }

    fn lock_cache(
        inner: &Inner,
    ) -> std::sync::MutexGuard<'_, std::collections::BTreeMap<PathBuf, Entry>> {
        inner
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Mark checkouts touched by `event` dirty and admit the reads that are already due.
    fn observe(inner: &Weak<Inner>, event: &notify::Result<notify::Event>) {
        let Some(inner) = inner.upgrade() else {
            return;
        };
        let (paths, rescan) = match event {
            Ok(event) if matches!(event.kind, EventKind::Access(_)) => return,
            Ok(event) => (event.paths.as_slice(), event.need_rescan()),
            Err(_) => (&[][..], true),
        };
        let mut admitted = Vec::new();
        {
            let mut cache = lock_cache(&inner);
            for (cwd, entry) in cache.iter_mut() {
                if !entry.watched || entry.git_read.running {
                    continue;
                }
                // Already dirty entries skip path matching, which dominates during event storms.
                if !entry.git_read.dirty {
                    let Some(roots) = &entry.roots else {
                        continue;
                    };
                    if !rescan && !paths.iter().any(|path| roots.affects(path)) {
                        continue;
                    }
                    entry.git_read.dirty = true;
                }
                let refresh = inner.git_refresh(entry);
                if entry.accessed_at.elapsed() < DEMAND_TTL && entry.git_read.due(refresh) {
                    entry.git_read.start();
                    admitted.push(cwd.clone());
                }
            }
        }
        for cwd in admitted {
            reads::enqueue(&inner, Job::Git(cwd));
        }
    }
}

#[cfg(test)]
mod tests;
