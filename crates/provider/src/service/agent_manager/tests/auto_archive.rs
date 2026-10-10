use super::*;

use domain::workspace::worktrees::{
    CreatedWorktreeWorkspace, WorktreeCreation, WorktreeCreationError,
};
use model::workspace::worktrees::WorktreeProvisioning;

#[derive(Debug)]
struct WorktreeCleanup {
    client: FakeClient,
    attempts: Mutex<usize>,
}

impl WorktreeProvisioning for WorktreeCleanup {
    fn prepare_directory(
        &self,
        _: &str,
        _: &domain::workspace::worktrees::DirectoryGit,
    ) -> Result<(), WorktreeCreationError> {
        panic!("cleanup must not change the source branch");
    }

    fn create(
        &self,
        _: &WorktreeCreation,
        _: &str,
    ) -> Result<CreatedWorktreeWorkspace, WorktreeCreationError> {
        panic!("cleanup must not provision another checkout");
    }

    fn archive(&self, workspace: &str, _: &str) -> Result<(), WorktreeCreationError> {
        assert_eq!(workspace, "workspace");
        assert_eq!(self.client.0.lock().unwrap().close_calls, 2);
        let mut attempts = self.attempts.lock().unwrap();
        *attempts += 1;
        if *attempts == 1 {
            return Err(WorktreeCreationError {
                code: "registry_io",
                message: "injected cleanup failure".into(),
            });
        }
        Ok(())
    }
}

#[tokio::test]
async fn worktree_auto_archive_closes_all_workspace_writers_and_retries_cleanup() {
    let (mut manager, registry, client) = make_manager();
    for id in ["agent-1", "agent-2"] {
        manager
            .create(
                id,
                &spec(),
                AgentRegistration {
                    workspace_id: Some("workspace".into()),
                    ..AgentRegistration::default()
                },
            )
            .await
            .unwrap();
    }
    let mut other = registry.get("agent-1").unwrap().unwrap();
    other.id = "other-workspace".into();
    other.workspace_id = Some("other".into());
    other
        .labels
        .insert("paseo.parent-agent-id".into(), "agent-1".into());
    registry.upsert(&other).unwrap();
    let cleanup = Arc::new(WorktreeCleanup {
        client: client.clone(),
        attempts: Mutex::new(0),
    });
    manager.auto_archive_worktree_on_finish("agent-1".into(), "workspace".into(), cleanup.clone());
    manager.send("agent-1", "finish").await.unwrap();
    client
        .0
        .lock()
        .unwrap()
        .events
        .push_back(AgentTurnEvent::Completed(None));
    assert_eq!(manager.poll().await, Err(AgentManagerError::Registry));
    assert!(manager.live.is_empty());
    assert!(
        registry
            .get("agent-2")
            .unwrap()
            .unwrap()
            .archived_at
            .is_some()
    );
    let other = registry.get("other-workspace").unwrap().unwrap();
    assert!(other.archived_at.is_none());
    assert!(!other.labels.contains_key("paseo.parent-agent-id"));
    manager.poll().await.unwrap();
    manager.poll().await.unwrap();
    assert_eq!(*cleanup.attempts.lock().unwrap(), 2);
    assert_eq!(client.0.lock().unwrap().close_calls, 2);
}

#[tokio::test]
async fn auto_archive_runs_once_after_each_terminal_outcome_and_can_be_restored() {
    for event in [
        AgentTurnEvent::Completed(Some("done".into())),
        AgentTurnEvent::Failed,
        AgentTurnEvent::Cancelled,
    ] {
        let (mut manager, registry, client) = make_manager();
        manager
            .create("agent-1", &spec(), AgentRegistration::default())
            .await
            .unwrap();
        manager.auto_archive_on_finish("agent-1".into());
        manager.poll().await.unwrap();
        assert!(
            registry
                .get("agent-1")
                .unwrap()
                .unwrap()
                .archived_at
                .is_none()
        );
        manager.send("agent-1", "hello").await.unwrap();
        client.0.lock().unwrap().events.push_back(event);
        manager.poll().await.unwrap();
        let archived = registry.get("agent-1").unwrap().unwrap();
        assert!(archived.archived_at.is_some());
        assert!(!archived.requires_attention);
        assert!(manager.live.is_empty());
        assert_eq!(client.0.lock().unwrap().close_calls, 1);
        manager.poll().await.unwrap();
        assert_eq!(client.0.lock().unwrap().close_calls, 1);
        manager
            .restore("agent-1", &crate::protocol::resume::Overrides::default())
            .await
            .unwrap();
        manager.send("agent-1", "again").await.unwrap();
        client
            .0
            .lock()
            .unwrap()
            .events
            .push_back(AgentTurnEvent::Completed(None));
        manager.poll().await.unwrap();
        assert!(
            registry
                .get("agent-1")
                .unwrap()
                .unwrap()
                .archived_at
                .is_none()
        );
    }
}

#[tokio::test]
async fn auto_archive_retries_native_cleanup_without_replaying_the_completed_turn() {
    let (mut manager, registry, client) = make_manager();
    manager
        .create("agent-1", &spec(), AgentRegistration::default())
        .await
        .unwrap();
    manager.auto_archive_on_finish("agent-1".into());
    manager.send("agent-1", "hello").await.unwrap();
    client
        .0
        .lock()
        .unwrap()
        .events
        .push_back(AgentTurnEvent::Completed(None));
    client.0.lock().unwrap().fail_close = true;
    assert_eq!(manager.poll().await, Err(AgentManagerError::Session));
    assert!(
        registry
            .get("agent-1")
            .unwrap()
            .unwrap()
            .archived_at
            .is_some()
    );
    assert!(manager.live.contains_key("agent-1"));
    client.0.lock().unwrap().fail_close = false;
    manager.poll().await.unwrap();
    assert!(manager.live.is_empty());
    assert_eq!(client.0.lock().unwrap().start_calls, 1);
}

#[tokio::test]
async fn deleting_an_armed_agent_removes_the_pending_action_without_resurrection() {
    let (mut manager, registry, _) = make_manager();
    manager
        .create("agent-1", &spec(), AgentRegistration::default())
        .await
        .unwrap();
    manager.auto_archive_on_finish("agent-1".into());
    registry.remove("agent-1").unwrap();
    manager.reconcile().await.unwrap();
    manager.poll().await.unwrap();
    assert!(registry.list().unwrap().is_empty());
    assert!(manager.live.is_empty());
}

#[tokio::test]
async fn queued_retirement_wakes_the_scheduler_once_and_failures_stay_retryable() {
    let owners = ownership::Owners::default();
    let (mut manager, _, client) = make_manager();
    manager.owner = Some(owners.owner(owners.agent("agent-1").unwrap()));
    manager
        .create("agent-1", &spec(), AgentRegistration::default())
        .await
        .unwrap();
    manager.auto_archive_on_finish("agent-1".into());
    assert!(!owners.has_idle_retirements());
    manager.send("agent-1", "hello").await.unwrap();
    client
        .0
        .lock()
        .unwrap()
        .events
        .push_back(AgentTurnEvent::Completed(None));
    manager.poll().await.unwrap();
    // The wakeup is retained until the scheduler awaits it.
    tokio::time::timeout(std::time::Duration::from_secs(1), owners.retired())
        .await
        .unwrap();
    assert!(owners.has_idle_retirements());
    let taken = owners.retirements().unwrap();
    assert_eq!(taken.len(), 1);
    assert!(!owners.has_idle_retirements());
    owners.finish_retirement("agent-1", false);
    assert!(owners.has_idle_retirements());
    owners.retirements().unwrap();
    owners.finish_retirement("agent-1", true);
    assert!(!owners.has_idle_retirements());
}
