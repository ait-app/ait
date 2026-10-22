use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::sync::Arc;

use domain::agent_runtime::StoredAgentConfig;
use domain::agent_runtime::registry::AgentRuntimeRegistry;
use model::workspace::registry::{ProjectRegistry, WorkspaceRegistry};
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
use persistence::storage::registry::{FileBackedProjectRegistry, FileBackedWorkspaceRegistry};
use serde_json::{Value, json};

use crate::local::codex::CodexClient;
use crate::ports::agent_session::AgentSessionSpec;
use crate::service::agent_execution::{AgentExecution, ExecutionDependencies};
use crate::service::agent_manager::AgentManager;
use crate::service::agent_runtime::AgentRuntimeDirectory;

pub(crate) struct Fixture {
    pub(crate) root: tempfile::TempDir,
    pub(crate) program: PathBuf,
    pub(crate) cwd: PathBuf,
}

impl Fixture {
    pub(crate) fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let program = root.path().join("codex");
        // Keep the executable immutable while concurrent tests spawn native processes.
        // The link also scopes optional diagnostic cwd overrides to this fixture.
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/codex_app_server.py");
        symlink(source, &program).unwrap();
        let cwd = root.path().join("work");
        std::fs::create_dir(&cwd).unwrap();
        Self { root, program, cwd }
    }

    pub(crate) fn spec(&self) -> AgentSessionSpec {
        AgentSessionSpec {
            provider: "codex".to_owned(),
            cwd: self.cwd.to_str().unwrap().to_owned(),
            config: StoredAgentConfig::default(),
        }
    }

    pub(crate) fn client(&self) -> CodexClient {
        CodexClient::new(self.program.clone())
    }

    pub(crate) fn mode(&self, mode: &str) {
        std::fs::write(self.cwd.join("behavior"), mode).unwrap();
    }

    pub(crate) fn requests(&self) -> Vec<Value> {
        std::fs::read_to_string(self.cwd.join("native-requests.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

/// Start a worker with `fixture` storage, shared `creations`, and the injected native `client`.
/// Returns the execution handle and its durable Agent registry; panics on fixture setup errors.
pub(crate) fn worker_with_client(
    fixture: &Fixture,
    creations: model::creation::Creations,
    client: Box<dyn crate::ports::agent_session::AgentClient>,
) -> (AgentExecution, FileBackedAgentRuntimeRegistry) {
    let registry = FileBackedAgentRuntimeRegistry::new(fixture.root.path().join("agents.json"));
    registry.initialize().unwrap();
    let projects = FileBackedProjectRegistry::new(fixture.root.path().join("projects.json"));
    projects.initialize().unwrap();
    let workspaces = FileBackedWorkspaceRegistry::new(fixture.root.path().join("workspaces.json"));
    workspaces.initialize().unwrap();
    projects.upsert(&serde_json::from_value(json!({
        "projectId":"project-1","rootPath":fixture.cwd,"kind":"non_git","displayName":"project",
        "createdAt":"2026-09-24T00:00:00Z","updatedAt":"2026-09-24T00:00:00Z","archivedAt":null
    })).unwrap()).unwrap();
    workspaces
        .upsert(
            &serde_json::from_value(json!({
                "workspaceId":"wks_0123456789abcdef","projectId":"project-1","cwd":fixture.cwd,
                "kind":"local_checkout","displayName":"main","createdAt":"2026-09-24T00:00:00Z",
                "updatedAt":"2026-09-24T00:00:00Z","archivedAt":null
            }))
            .unwrap(),
            domain::workspace::registry::WorkspaceMutationContext::default(),
        )
        .unwrap();
    let mut manager = AgentManager::new(Box::new(registry.clone()))
        .with_creations(creations)
        .with_timeline(
            crate::storage::timeline::Timeline::open(&fixture.root.path().join("timeline.sqlite"))
                .unwrap(),
        );
    manager.register_client(client).unwrap();
    let worker = AgentExecution::spawn(ExecutionDependencies {
        workspace_automation: None,
        manager,
        directory: AgentRuntimeDirectory::new(
            Box::new(registry.clone()),
            Box::new(workspaces.clone()),
            Box::new(projects.clone()),
        ),
        registry: Box::new(registry.clone()),
        workspaces: Box::new(workspaces),
        lifetime: Arc::new(()),
        import_directory: None,
        projects: Box::new(projects),
    })
    .unwrap();
    (worker, registry)
}

/// Execute `method` with `params` and return its result, failing the test on errors or timeout.
pub(crate) async fn request(execution: &AgentExecution, method: &str, params: Value) -> Value {
    tokio::time::timeout(
        std::time::Duration::from_secs(30),
        execution.execute(method, params),
    )
    .await
    .unwrap_or_else(|_| panic!("{method} timed out"))
    .unwrap_or_else(|error| panic!("{method}: {error:?}"))
}
