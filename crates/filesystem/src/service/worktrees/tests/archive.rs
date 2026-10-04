use super::*;

#[test]
fn archive_rpc_keeps_cleanup_failure_inline_and_a_retry_can_finish() {
    use crate::rpc::worktrees::execute;
    let managed = Managed::default();
    let mut service = service(&Projects::default(), &Workspaces::default(), &managed);
    managed.state.lock().unwrap().fail_remove = true;
    let params = serde_json::json!({"worktreePath":"/managed/hash/topic","scope":"worktree"});
    let failed = execute(
        &mut service,
        "workspace.worktree.archive.request",
        params.clone(),
    )
    .unwrap();
    assert_eq!(failed.value["success"], false);
    assert!(
        failed.value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("cleanup denied")
    );
    assert!(failed.event.is_none());
    managed.state.lock().unwrap().fail_remove = false;
    let retried = execute(&mut service, "workspace.worktree.archive.request", params).unwrap();
    assert_eq!(retried.value["success"], true);
    assert!(retried.value["error"].is_null());
    assert!(matches!(
        execute(&mut service, "unknown", serde_json::json!({})),
        Err(crate::rpc::ErrorCode::MethodNotFound)
    ));
}

#[test]
fn archive_resolves_legacy_selectors_and_rejects_incomplete_or_missing_targets() {
    for (slug, branch, root) in [
        (Some("topic"), None, Some("/repo")),
        (None, Some("topic"), Some("/repo")),
    ] {
        let managed = Managed::default();
        let service = service(&Projects::default(), &Workspaces::default(), &managed);
        let input = ArchiveWorktree {
            worktree_path: None,
            worktree_slug: slug.map(str::to_owned),
            branch_name: branch.map(str::to_owned),
            repo_root: root.map(str::to_owned),
            ..archive_input(ArchiveScope::Worktree)
        };
        service.archive(&input, "archived").unwrap();
        assert_eq!(
            managed.state.lock().unwrap().removed,
            ["/managed/hash/topic"]
        );
    }
    for (slug, branch, root, workspace_id) in [
        (Some("topic"), None, None, None),
        (None, Some("missing"), Some("/repo"), None),
        (None, None, None, Some("missing")),
        (None, None, None, None),
    ] {
        let managed = Managed::default();
        let service = service(&Projects::default(), &Workspaces::default(), &managed);
        let input = ArchiveWorktree {
            worktree_path: None,
            worktree_slug: slug.map(str::to_owned),
            branch_name: branch.map(str::to_owned),
            repo_root: root.map(str::to_owned),
            workspace_id: workspace_id.map(str::to_owned),
            scope: ArchiveScope::Worktree,
        };
        assert!(service.archive(&input, "archived").is_err());
        assert!(managed.state.lock().unwrap().removed.is_empty());
    }
}

#[test]
fn failed_registration_reports_failed_rollback_and_keeps_the_original_cause() {
    let managed = Managed::default();
    managed.state.lock().unwrap().fail_remove = true;
    let service = service(&Projects::default(), &Workspaces::default(), &managed);
    let error = service
        .create(&create_input(Some("missing")), "now")
        .unwrap_err();
    assert_eq!(error.kind(), WorktreeFailureKind::Other);
    let WorktreesError::Rollback { cause, rollback } = error else {
        panic!("rollback failure was lost")
    };
    assert!(matches!(*cause, WorktreesError::UnknownProject(_)));
    assert_eq!(rollback, WorktreeError::Io("cleanup denied".into()));
}

#[derive(Debug, Clone, Default)]
struct Cleanup {
    failed: Arc<std::sync::atomic::AtomicBool>,
    calls: Arc<Mutex<Vec<Vec<String>>>>,
}

impl crate::ports::worktrees::WorktreeArchiveCleanup for Cleanup {
    fn close_workspaces(&self, ids: &[String]) -> Result<(), WorktreeError> {
        self.calls.lock().unwrap().push(ids.to_vec());
        if self.failed.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(WorktreeError::Io("cleanup failed".into()));
        }
        Ok(())
    }
}

#[test]
fn archive_cleanup_failure_retains_checkout_and_retries_the_archived_workspace() {
    let workspaces = Workspaces::default();
    workspaces.records.lock().unwrap().push(workspace(
        "one",
        "/managed/hash/topic",
        "/managed/hash/topic",
        "prj",
    ));
    let managed = Managed::default();
    let mut service = service(&Projects::default(), &workspaces, &managed);
    let cleanup = Cleanup::default();
    service.set_archive_cleanup(Box::new(cleanup.clone()));
    cleanup
        .failed
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let input = archive_input(ArchiveScope::Worktree);
    assert!(service.archive(&input, "archived").is_err());
    assert!(managed.state.lock().unwrap().removed.is_empty());
    cleanup
        .failed
        .store(false, std::sync::atomic::Ordering::SeqCst);
    service.archive(&input, "retried").unwrap();
    assert_eq!(managed.state.lock().unwrap().removed.len(), 1);
    assert_eq!(
        *cleanup.calls.lock().unwrap(),
        vec![vec!["one"], vec!["one"]]
    );
}

#[test]
fn prepared_archive_retains_checkout_until_resource_cleanup_and_retries_archived_ids() {
    let workspaces = Workspaces::default();
    workspaces.records.lock().unwrap().push(workspace(
        "one",
        "/managed/hash/topic",
        "/managed/hash/topic",
        "prj",
    ));
    let managed = Managed::default();
    let service = service(&Projects::default(), &workspaces, &managed);
    let input = ArchiveWorktree {
        workspace_id: Some("one".into()),
        worktree_path: None,
        ..archive_input(ArchiveScope::Workspace)
    };
    let pending = service.begin_archive(&input, "archived").unwrap();
    assert_eq!(pending.workspace_ids, ["one"]);
    assert!(
        workspaces
            .get("one")
            .unwrap()
            .unwrap()
            .archived_at
            .is_some()
    );
    assert!(managed.state.lock().unwrap().removed.is_empty());
    // A host cleanup failure leaves an archived record and the original checkout for retry.
    drop(pending);
    let retried = service.begin_archive(&input, "retried").unwrap();
    assert_eq!(retried.workspace_ids, ["one"]);
    assert!(
        service
            .finish_archive(retried)
            .unwrap()
            .workspace_ids
            .is_empty()
    );
    assert_eq!(managed.state.lock().unwrap().removed.len(), 1);
}

#[test]
fn a_new_reference_during_worktree_wide_archive_prevents_checkout_removal() {
    let workspaces = Workspaces::default();
    workspaces.records.lock().unwrap().push(workspace(
        "one",
        "/managed/hash/topic",
        "/managed/hash/topic",
        "prj",
    ));
    let managed = Managed::default();
    let service = service(&Projects::default(), &workspaces, &managed);
    let input = archive_input(ArchiveScope::Worktree);
    let pending = service.begin_archive(&input, "archived").unwrap();
    workspaces.records.lock().unwrap().push(workspace(
        "new",
        "/managed/hash/topic",
        "/managed/hash/topic",
        "prj",
    ));
    assert!(service.finish_archive(pending).is_err());
    assert!(managed.state.lock().unwrap().removed.is_empty());
    let pending = service.begin_archive(&input, "retried").unwrap();
    assert_eq!(pending.workspace_ids, ["one", "new"]);
    service.finish_archive(pending).unwrap();
    assert_eq!(managed.state.lock().unwrap().removed.len(), 1);
}
