use super::*;
use crate::model::registry::PersistedWorkspaceRecord;
use crate::ports::registry::WorkspaceMutationContext;
use crate::storage::registry::FileBackedWorkspaceRegistry;
use model::summary::{SummaryError, SummaryFuture};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Default)]
struct Generator {
    started: tokio::sync::Notify,
    release: tokio::sync::Notify,
    calls: AtomicUsize,
}
impl SummarySource for Generator {
    fn generate(&self, request: SummaryRequest) -> SummaryFuture<'_> {
        Box::pin(async move {
            assert_eq!(request.kind, SummaryKind::BranchName);
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.started.notify_one();
            self.release.notified().await;
            if request.context == "fail" {
                Err(SummaryError::Unavailable)
            } else {
                Ok(json!({"title":"Generated title","branch":"fix/title"}))
            }
        })
    }
    fn shutdown(&self) {}
}
#[derive(Debug, Default)]
struct Branches(AtomicUsize);
impl WorkspaceBranchNamer for Branches {
    fn rename(&self, _: &str, expected: &str, desired: &str) -> Option<String> {
        assert_eq!(expected, "placeholder");
        self.0.fetch_add(1, Ordering::SeqCst);
        Some(desired.into())
    }
}
fn record() -> PersistedWorkspaceRecord {
    serde_json::from_value(json!({"workspaceId":"wks_test","projectId":"prj_test","cwd":"/repo","kind":"worktree","displayName":"placeholder","title":"Prompt","branch":"placeholder","autoName":{"placeholderBranch":"placeholder"},"createdAt":"initial","updatedAt":"initial","archivedAt":null})).unwrap()
}

#[tokio::test]
async fn applies_only_owned_fields_and_coalesces_duplicate_work() {
    let root = tempfile::tempdir().unwrap();
    let registry = Arc::new(FileBackedWorkspaceRegistry::new(
        root.path().join("workspaces.json"),
    ));
    registry
        .upsert(&record(), WorkspaceMutationContext::default())
        .unwrap();
    let generator = Arc::new(Generator::default());
    let branches = Arc::new(Branches::default());
    let names = WorkspaceNames::new(registry.clone(), generator.clone(), branches.clone());
    model::workspace::lifecycle::WorkspaceNaming::schedule(
        &names,
        "wks_test".into(),
        "source".into(),
        None,
    );
    generator.started.notified().await;
    names.schedule("wks_test".into(), "duplicate".into(), None);
    registry
        .update("wks_test", &|before| {
            let mut next = before.clone();
            next.labels = Some(vec!["keep".into()]);
            next
        })
        .unwrap();
    generator.release.notify_one();
    names.inner.tasks.close();
    names.wait_closed().await;
    let result = registry.get("wks_test").unwrap().unwrap();
    assert_eq!(result.title.as_deref(), Some("Generated title"));
    assert_eq!(result.branch.as_deref(), Some("fix/title"));
    assert_eq!(result.labels, Some(vec!["keep".into()]));
    assert!(result.auto_name.is_none());
    assert_eq!(generator.calls.load(Ordering::SeqCst), 1);
    assert_eq!(branches.0.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn manual_same_title_archive_and_failures_preserve_workspace() {
    for change in ["manual", "archive", "fail", "directory"] {
        let root = tempfile::tempdir().unwrap();
        let registry = Arc::new(FileBackedWorkspaceRegistry::new(
            root.path().join("workspaces.json"),
        ));
        let mut before = record();
        if change == "directory" {
            before.auto_name.as_mut().unwrap().placeholder_branch = None;
        }
        registry
            .upsert(&before, WorkspaceMutationContext::default())
            .unwrap();
        let generator = Arc::new(Generator::default());
        let branches = Arc::new(Branches::default());
        let names = WorkspaceNames::new(registry.clone(), generator.clone(), branches.clone());
        names.schedule("wks_test".into(), change.into(), None);
        generator.started.notified().await;
        registry
            .update("wks_test", &|before| {
                let mut next = before.clone();
                if change == "manual" {
                    next.auto_name = None;
                }
                if change == "archive" {
                    next.archived_at = Some("archived".into());
                }
                next
            })
            .unwrap();
        generator.release.notify_one();
        names.inner.tasks.close();
        names.wait_closed().await;
        let after = registry.get("wks_test").unwrap().unwrap();
        assert_eq!(
            after.title.as_deref(),
            Some(if change == "directory" {
                "Generated title"
            } else {
                "Prompt"
            })
        );
        assert_eq!(branches.0.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn shutdown_cancels_waiting_generation_and_refuses_new_work() {
    let root = tempfile::tempdir().unwrap();
    let registry = Arc::new(FileBackedWorkspaceRegistry::new(
        root.path().join("workspaces.json"),
    ));
    registry
        .upsert(&record(), WorkspaceMutationContext::default())
        .unwrap();
    let generator = Arc::new(Generator::default());
    let names = WorkspaceNames::new(
        registry.clone(),
        generator.clone(),
        Arc::new(Branches::default()),
    );
    names.schedule("wks_test".into(), "source".into(), None);
    generator.started.notified().await;
    names.shutdown();
    tokio::time::timeout(std::time::Duration::from_secs(1), names.wait_closed())
        .await
        .unwrap();
    names.schedule("wks_test".into(), "later".into(), None);
    assert_eq!(generator.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        registry.get("wks_test").unwrap().unwrap().title,
        record().title
    );
}

#[test]
fn empty_creation_context_waits_for_a_real_prompt_or_attachment() {
    assert!(first_agent_source(None, &[]).is_none());
    assert!(first_agent_source(Some(" \n "), &[]).is_none());
    assert!(
        first_agent_source(Some("Fix titles"), &[])
            .unwrap()
            .contains("Fix titles")
    );
    assert!(
        first_agent_source(None, &[json!({"name":"review.txt"})])
            .unwrap()
            .contains("review.txt")
    );
}
