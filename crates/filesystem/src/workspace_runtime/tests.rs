//! Offline cache and Forge regressions with local Git and a controlled CLI fixture.

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::json;

use super::*;

#[cfg(unix)]
mod cli;

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repository(root: &Path) -> PathBuf {
    let repo = root.join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    fs::write(repo.join("tracked.txt"), "base\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "base"]);
    git(&repo, &["checkout", "-b", "feature"]);
    repo
}

fn wait_for(
    source: &LocalWorkspaceRuntime,
    cwd: &Path,
    predicate: impl Fn(&WorkspaceRuntimeSnapshot) -> bool,
) -> WorkspaceRuntimeSnapshot {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let snapshot = source.snapshot(cwd.to_str().unwrap());
        if predicate(&snapshot) {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "cache did not converge: {snapshot:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Wait until Git workers finish, including watcher registration after a published read.
fn settle(source: &LocalWorkspaceRuntime) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while source.inner.git_reads.load(Ordering::Relaxed) != 0 {
        assert!(Instant::now() < deadline, "Git reads did not finish");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn expire(source: &LocalWorkspaceRuntime, cwd: &Path, is_forge: bool) {
    let mut cache = source.inner.cache.lock().unwrap();
    let entry = cache.get_mut(&cwd.canonicalize().unwrap()).unwrap();
    if is_forge {
        entry.forge_read.completed_at = None;
    } else {
        entry.git_read.completed_at = None;
    }
}

#[test]
fn workspace_runtime_cache_coalesces_clones_and_refreshes_local_edits() {
    let root = tempfile::tempdir().unwrap();
    let repo = repository(root.path());
    let source = LocalWorkspaceRuntime::new(
        LocalCheckout::new(root.path().join("managed")),
        LocalForge::new(),
    );
    assert_eq!(
        source.snapshot(repo.to_str().unwrap()),
        WorkspaceRuntimeSnapshot::default()
    );
    let clone = source.clone();
    let cached = wait_for(&clone, &repo, |snapshot| snapshot.git.is_some());
    assert_eq!(
        cached.git.unwrap().current_branch.as_deref(),
        Some("feature")
    );
    settle(&source);
    for _ in 0..20 {
        clone.snapshot(repo.to_str().unwrap());
    }
    let cache = source.inner.cache.lock().unwrap();
    assert_eq!(cache.len(), 1);
    assert!(
        cache.values().all(|entry| !entry.git_read.running),
        "fresh snapshots coalesce without admitting reads"
    );
    drop(cache);

    fs::write(repo.join("tracked.txt"), "base\nnew\n").unwrap();
    expire(&source, &repo, false);
    let changed = wait_for(&source, &repo, |snapshot| {
        snapshot
            .git
            .as_ref()
            .is_some_and(|git| git.diff_stat.is_some())
    });
    assert_eq!(changed.git.unwrap().diff_stat.unwrap().additions, 1);
    fs::remove_dir_all(&repo).unwrap();
    // Missing directories clear stale checkout facts without failing the Workspace directory.
    source
        .inner
        .cache
        .lock()
        .unwrap()
        .values_mut()
        .for_each(|entry| entry.git_read.completed_at = None);
    wait_for(&source, &repo, |snapshot| snapshot.git.is_none());
}

#[test]
fn workspace_runtime_clears_cached_diff_after_remote_base_advances() {
    let root = tempfile::tempdir().unwrap();
    let repo = repository(root.path());
    git(&repo, &["update-ref", "refs/remotes/origin/main", "main"]);
    fs::write(repo.join("tracked.txt"), "base\nnew\n").unwrap();
    git(&repo, &["commit", "-am", "feature"]);
    let source = LocalWorkspaceRuntime::new(
        LocalCheckout::new(root.path().join("managed")),
        LocalForge::new(),
    );
    let changed = wait_for(&source, &repo, |snapshot| {
        snapshot
            .git
            .as_ref()
            .is_some_and(|git| git.diff_stat.is_some())
    });
    assert_eq!(changed.git.unwrap().diff_stat.unwrap().additions, 1);

    git(&repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    // Leave HEAD untouched and let the normal cache expiry discover the updated base.
    let merged = wait_for(&source, &repo, |snapshot| {
        snapshot
            .git
            .as_ref()
            .is_some_and(|git| git.diff_stat.is_none() && git.is_dirty == Some(false))
    });
    assert!(merged.git.unwrap().diff_stat.is_none());
}

#[test]
fn workspace_runtime_read_permits_are_bounded_and_released_on_drop() {
    let active = Arc::<AtomicUsize>::default();
    let first = Permit::acquire(&active).unwrap();
    let second = Permit::acquire(&active).unwrap();
    assert!(Permit::acquire(&active).is_none());
    drop(first);
    assert!(Permit::acquire(&active).is_some());
    drop(second);
    assert_eq!(active.load(Ordering::Relaxed), 0);
}

#[test]
fn workspace_runtime_refreshes_every_queued_checkout_without_repeated_snapshot_reads() {
    let root = tempfile::tempdir().unwrap();
    let source = LocalWorkspaceRuntime::new(
        LocalCheckout::new(root.path().join("managed")),
        LocalForge::new(),
    );
    let repos: Vec<_> = (0..8)
        .map(|index| {
            let directory = root.path().join(index.to_string());
            fs::create_dir(&directory).unwrap();
            repository(&directory)
        })
        .collect();
    for repo in &repos {
        source.snapshot(repo.to_str().unwrap());
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let mut cache = source.inner.cache.lock().unwrap();
        if cache.values().all(|entry| entry.value.git.is_some()) {
            assert_eq!(cache.len(), repos.len());
            break;
        }
        // Keep demand alive without enqueueing reads; a loaded host can exceed DEMAND_TTL.
        for entry in cache.values_mut() {
            entry.accessed_at = Instant::now();
        }
        drop(cache);
        assert!(Instant::now() < deadline, "queued checkouts were starved");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn workspace_runtime_forge_availability_does_not_fabricate_a_pull_request() {
    use crate::forge::ports::{
        ForgeAuthState, ForgeFailureKind, ForgeRuntimeError, PullRequestStatusRead,
    };
    for auth_state in [
        ForgeAuthState::Unauthenticated,
        ForgeAuthState::CliMissing,
        ForgeAuthState::NoRemote,
    ] {
        let read = forge::snapshot(Ok(PullRequestStatusRead {
            status: None,
            auth_state,
            forge: Some("gitlab".to_owned()),
        }));
        assert!(!read.features_enabled);
        assert!(read.pull_request.is_none());
    }
    let read = forge::snapshot(Err(ForgeRuntimeError {
        forge: Some("github".to_owned()),
        kind: ForgeFailureKind::Unknown,
        message: "network unavailable".to_owned(),
    }));
    assert!(read.features_enabled);
    assert_eq!(read.error.as_deref(), Some("network unavailable"));
}

#[test]
fn workspace_runtime_refresh_backs_off_while_unchanged_and_resets_on_change() {
    let intervals: Vec<_> = (0..6)
        .map(|unchanged| GIT_REFRESH.interval(unchanged))
        .collect();
    assert_eq!(
        intervals,
        [2, 4, 8, 16, 30, 30].map(Duration::from_secs).to_vec()
    );
    assert_eq!(GIT_REFRESH.interval(u32::MAX), GIT_REFRESH.max);
    assert_eq!(FORGE_REFRESH.interval(0), FORGE_REFRESH.base);

    let mut read = ReadState::default();
    assert!(read.due(GIT_REFRESH), "never-read checkouts are due");
    read.finish(false);
    read.finish(false);
    assert_eq!(read.unchanged, 2);
    assert!(!read.due(GIT_REFRESH), "a fresh read is not due");
    read.completed_at = Instant::now().checked_sub(Duration::from_secs(5));
    assert!(!read.due(GIT_REFRESH), "two idle reads wait eight seconds");
    read.completed_at = Instant::now().checked_sub(Duration::from_secs(8));
    assert!(read.due(GIT_REFRESH));
    read.finish(true);
    assert_eq!(read.unchanged, 0);
}

#[cfg(target_os = "macos")]
#[test]
fn workspace_runtime_file_events_refresh_idle_checkouts_without_polling() {
    let root = tempfile::tempdir().unwrap();
    let repo = repository(root.path());
    let source = LocalWorkspaceRuntime::new(
        LocalCheckout::new(root.path().join("managed")),
        LocalForge::new(),
    );
    wait_for(&source, &repo, |snapshot| snapshot.git.is_some());
    settle(&source);
    let cwd = repo.canonicalize().unwrap();
    {
        let mut cache = source.inner.cache.lock().unwrap();
        let entry = cache.get_mut(&cwd).unwrap();
        assert!(entry.roots.is_some() && entry.watched);
        // Push the polling backoff far out, so only a file event can admit the next read.
        entry.git_read.unchanged = 16;
        entry.git_read.completed_at = Instant::now().checked_sub(Duration::from_secs(3));
    }
    assert_eq!(
        source
            .inner
            .git_refresh(&source.inner.cache.lock().unwrap()[&cwd])
            .max,
        WATCHED_GIT_REFRESH.max
    );

    fs::write(repo.join("tracked.txt"), "base\nwatched\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let mut cache = source.inner.cache.lock().unwrap();
        let entry = cache.get_mut(&cwd).unwrap();
        if entry
            .value
            .git
            .as_ref()
            .and_then(|git| git.diff_stat)
            .is_some_and(|stat| stat.additions == 1)
        {
            break;
        }
        entry.accessed_at = Instant::now();
        drop(cache);
        assert!(
            Instant::now() < deadline,
            "file event did not refresh the checkout"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn workspace_runtime_dirty_checkouts_refresh_at_the_base_interval() {
    let mut read = ReadState {
        unchanged: 16,
        completed_at: Instant::now().checked_sub(Duration::from_secs(3)),
        ..ReadState::default()
    };
    assert!(
        !read.due(WATCHED_GIT_REFRESH),
        "idle watched checkouts wait for events"
    );
    read.dirty = true;
    assert!(read.due(WATCHED_GIT_REFRESH));
    read.start();
    assert!(read.running && !read.dirty);
    assert!(!read.due(WATCHED_GIT_REFRESH));
}
