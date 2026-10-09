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
    for _ in 0..20 {
        clone.snapshot(repo.to_str().unwrap());
    }
    assert_eq!(source.inner.cache.lock().unwrap().len(), 1);
    assert_eq!(source.inner.git_reads.load(Ordering::Relaxed), 0);

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
        let cache = source.inner.cache.lock().unwrap();
        if cache.values().all(|entry| entry.value.git.is_some()) {
            assert_eq!(cache.len(), repos.len());
            break;
        }
        drop(cache);
        assert!(Instant::now() < deadline, "queued checkouts were starved");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn workspace_runtime_forge_availability_does_not_fabricate_a_pull_request() {
    use crate::forge::ports::forge::{
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
