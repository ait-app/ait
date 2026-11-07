use std::time::Instant;

use super::*;
use crate::workspace_runtime::{
    Entry, LocalCheckout, LocalForge, LocalWorkspaceRuntime, ReadState, WorkspaceRuntimeSnapshot,
};

#[test]
fn expired_or_removed_cache_entries_release_queued_reads_without_resurrecting_results() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().to_path_buf();
    let source = LocalWorkspaceRuntime::new(
        LocalCheckout::new(root.path().join("managed")),
        LocalForge::new(),
    );
    let entry = Entry {
        value: WorkspaceRuntimeSnapshot::default(),
        head: None,
        git_read: ReadState {
            running: true,
            ..ReadState::default()
        },
        forge_read: ReadState {
            running: true,
            ..ReadState::default()
        },
        accessed_at: Instant::now().checked_sub(DEMAND_TTL).unwrap(),
        roots: None,
        watched: false,
    };
    let identity = Identity::from_entry(&entry);
    source
        .inner
        .cache
        .lock()
        .unwrap()
        .insert(cwd.clone(), entry);
    for job in [
        Job::Git(cwd.clone()),
        Job::Forge(cwd.clone(), identity.clone()),
    ] {
        assert!(!has_demand(&source.inner, &job));
    }
    let mut cache = source.inner.cache.lock().unwrap();
    let expired = cache.get(&cwd).unwrap();
    assert!(!expired.git_read.running && !expired.forge_read.running);
    assert!(expired.git_read.completed_at.is_none() && expired.forge_read.completed_at.is_none());
    assert!(
        expired.git_read.dirty && expired.forge_read.dirty,
        "dropped reads stay due for the next reader"
    );
    cache.clear();
    drop(cache);
    assert!(!has_demand(&source.inner, &Job::Git(cwd.clone())));
    read_git(&source.inner, &cwd);
    read_forge(&source.inner, &cwd, &identity);
    assert!(source.inner.cache.lock().unwrap().is_empty());
}

fn idle_entry() -> Entry {
    Entry {
        value: WorkspaceRuntimeSnapshot::default(),
        head: None,
        git_read: ReadState {
            running: true,
            ..ReadState::default()
        },
        forge_read: ReadState::default(),
        accessed_at: Instant::now(),
        roots: None,
        watched: false,
    }
}

#[test]
fn unchanged_git_reads_back_off_without_waking_directory_observers() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().canonicalize().unwrap();
    let changes = model::changes::Changes::default();
    let mut observer = changes.subscribe();
    let source = LocalWorkspaceRuntime::new(
        LocalCheckout::new(root.path().join("managed")),
        LocalForge::new(),
    )
    .with_changes(changes);
    source
        .inner
        .cache
        .lock()
        .unwrap()
        .insert(cwd.clone(), idle_entry());

    read_git(&source.inner, &cwd);
    assert!(
        observer.has_changed().unwrap(),
        "first completion publishes"
    );
    observer.borrow_and_update();
    for expected in 1..=2 {
        read_git(&source.inner, &cwd);
        assert!(
            !observer.has_changed().unwrap(),
            "identical read stays silent"
        );
        assert_eq!(
            source.inner.cache.lock().unwrap()[&cwd].git_read.unchanged,
            expected
        );
    }

    source
        .inner
        .cache
        .lock()
        .unwrap()
        .get_mut(&cwd)
        .unwrap()
        .forge_read
        .unchanged = 3;
    std::fs::write(cwd.join("README"), "now a checkout\n").unwrap();
    let status = std::process::Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(&cwd)
        .status()
        .unwrap();
    assert!(status.success());
    read_git(&source.inner, &cwd);
    assert!(observer.has_changed().unwrap(), "changed facts publish");
    let cache = source.inner.cache.lock().unwrap();
    assert_eq!(cache[&cwd].git_read.unchanged, 0);
    assert_eq!(
        cache[&cwd].forge_read.unchanged, 0,
        "a new Git identity resets the Forge backoff"
    );
}

#[test]
fn unchanged_forge_reads_back_off_without_waking_directory_observers() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().canonicalize().unwrap();
    let changes = model::changes::Changes::default();
    let mut observer = changes.subscribe();
    let source = LocalWorkspaceRuntime::new(
        LocalCheckout::new(root.path().join("managed")),
        LocalForge::new(),
    )
    .with_changes(changes);
    let entry = idle_entry();
    let identity = Identity::from_entry(&entry);
    source
        .inner
        .cache
        .lock()
        .unwrap()
        .insert(cwd.clone(), entry);

    read_forge(&source.inner, &cwd, &identity);
    assert!(observer.has_changed().unwrap(), "first Forge facts publish");
    observer.borrow_and_update();
    for expected in 1..=2 {
        read_forge(&source.inner, &cwd, &identity);
        assert!(
            !observer.has_changed().unwrap(),
            "identical Forge read stays silent"
        );
        assert_eq!(
            source.inner.cache.lock().unwrap()[&cwd]
                .forge_read
                .unchanged,
            expected
        );
    }
}
