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
            completed_at: None,
        },
        forge_read: ReadState {
            running: true,
            completed_at: None,
        },
        accessed_at: Instant::now().checked_sub(DEMAND_TTL).unwrap(),
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
    cache.clear();
    drop(cache);
    assert!(!has_demand(&source.inner, &Job::Git(cwd.clone())));
    read_git(&source.inner, &cwd);
    read_forge(&source.inner, &cwd, &identity);
    assert!(source.inner.cache.lock().unwrap().is_empty());
}
