use std::path::Path;
use std::sync::Arc;

use domain::agent_runtime::StoredAgentConfig;
use domain::agent_runtime::registry::AgentRuntimeRegistry;
use domain::workspace::registry::WorkspaceMutationContext;
use model::workspace::registry::WorkspaceRegistry;
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
use persistence::storage::registry::FileBackedWorkspaceRegistry;
use serde_json::json;

use super::*;
use crate::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionError,
};

#[derive(Debug)]
struct Client {
    confines: bool,
}

#[async_trait::async_trait]
impl AgentClient for Client {
    fn supports_write_confinement(&self) -> bool {
        self.confines
    }

    fn provider(&self) -> &'static str {
        "codex"
    }

    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        Ok(true)
    }

    async fn create_session(
        &self,
        _spec: &AgentSessionSpec,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        Err(AgentSessionError::Unavailable)
    }

    async fn resume_session(
        &self,
        _handle: &domain::agent_runtime::AgentPersistenceHandle,
        _spec: &AgentSessionSpec,
        _purpose: AgentResumePurpose,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        Err(AgentSessionError::Unavailable)
    }
}

struct Fixture {
    root: tempfile::TempDir,
    workspaces: FileBackedWorkspaceRegistry,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("temporary directory");
        let workspaces = FileBackedWorkspaceRegistry::new(root.path().join("workspaces.json"));
        workspaces.initialize().expect("workspace registry");
        Self { root, workspaces }
    }

    fn workspace(&self, id: &str, sandbox: Option<&str>) {
        let record = serde_json::from_value(json!({
            "workspaceId": id,
            "projectId": "prj_1",
            "cwd": "/repo/wt/sub",
            "kind": "worktree",
            "displayName": "feature",
            "worktreeRoot": "/repo/wt",
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-01T00:00:00Z",
            "archivedAt": null,
            "sandbox": sandbox,
        }))
        .expect("valid workspace record");
        self.workspaces
            .upsert(&record, WorkspaceMutationContext::default())
            .expect("stored workspace");
    }

    fn manager(&self, confines: bool) -> AgentManager {
        let agents = FileBackedAgentRuntimeRegistry::new(self.root.path().join("agents.json"));
        agents.initialize().expect("agent registry");
        let mut manager =
            AgentManager::new(Box::new(agents)).with_workspaces(Arc::new(self.workspaces.clone()));
        manager
            .register_client(Box::new(Client { confines }))
            .expect("registered client");
        manager
    }
}

fn spec() -> AgentSessionSpec {
    AgentSessionSpec {
        provider: "codex".to_owned(),
        cwd: "/repo/wt/sub".to_owned(),
        config: StoredAgentConfig::default(),
        confinement: None,
    }
}

const SANDBOXED: &str = "wks_00000000000000a1";
const PLAIN: &str = "wks_00000000000000b2";

#[test]
fn launches_outside_a_sandboxed_workspace_stay_borrowed_and_unconfined() {
    let fixture = Fixture::new();
    fixture.workspace(PLAIN, None);
    let manager = fixture.manager(false);
    let spec = spec();

    for workspace in [None, Some(PLAIN), Some("wks_00000000000000ff")] {
        let launch = manager
            .confine(Cow::Borrowed(&spec), workspace)
            .expect("unconfined launch");
        assert!(matches!(launch, Cow::Borrowed(_)), "{workspace:?}");
        assert_eq!(launch.confinement, None);
    }
}

#[test]
fn sandboxed_workspace_attaches_its_worktree_root() {
    let fixture = Fixture::new();
    fixture.workspace(SANDBOXED, Some("worktree_writes"));
    let manager = fixture.manager(true);
    let spec = spec();

    let launch = manager
        .confine(Cow::Borrowed(&spec), Some(SANDBOXED))
        .expect("confined launch");

    let confinement = launch.confinement.as_ref().expect("confinement");
    assert_eq!(confinement.worktree_root(), Path::new("/repo/wt"));
    assert_eq!(
        spec.confinement, None,
        "the caller's settings are not changed"
    );
}

#[test]
fn provider_without_confinement_support_is_rejected() {
    let fixture = Fixture::new();
    fixture.workspace(SANDBOXED, Some("worktree_writes"));
    let manager = fixture.manager(false);

    let error = manager
        .confine(Cow::Owned(spec()), Some(SANDBOXED))
        .expect_err("unsupported provider");

    assert_eq!(
        error,
        AgentManagerError::ConfinementUnsupported("codex".to_owned())
    );
    assert_eq!(
        manager.confine_control(spec(), Some(SANDBOXED)).err(),
        Some(ErrorCode::UnsupportedCapability)
    );
}

#[test]
fn manager_without_a_workspace_registry_never_confines() {
    let root = tempfile::tempdir().expect("temporary directory");
    let agents = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    agents.initialize().expect("agent registry");
    let manager = AgentManager::new(Box::new(agents));

    let launch = manager
        .confine_control(spec(), Some(SANDBOXED))
        .expect("unconfined launch");

    assert_eq!(launch.confinement, None);
}

#[test]
fn unreadable_workspace_registry_fails_closed() {
    let fixture = Fixture::new();
    let manager = fixture.manager(true);
    std::fs::write(fixture.root.path().join("workspaces.json"), "not json")
        .expect("corrupt registry");
    let reloaded = FileBackedWorkspaceRegistry::new(fixture.root.path().join("workspaces.json"));
    let manager = manager.with_workspaces(Arc::new(reloaded));

    assert_eq!(
        manager.confine(Cow::Owned(spec()), Some(SANDBOXED)).err(),
        Some(AgentManagerError::Registry)
    );
    assert_eq!(
        manager.confine_control(spec(), Some(SANDBOXED)).err(),
        Some(ErrorCode::AgentIo)
    );
}
