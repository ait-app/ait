use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::sync::Arc;

use domain::agent_runtime::StoredAgentConfig;
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
use serde_json::Value;

use crate::local::codex::CodexClient;
use crate::ports::usage_ledger::{NoopUsageLedger, UsageLedger};
use crate::service::agent_execution::AgentExecution;
use provider_core::ports::agent_session::AgentSessionSpec;

pub(crate) mod codex_history;
pub(crate) mod usage_ledger;

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
            confinement: None,
        }
    }

    pub(crate) fn client(&self) -> CodexClient {
        CodexClient::new(self.program.clone()).with_fixture_media()
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
    client: Box<dyn provider_core::ports::agent_session::AgentClient>,
) -> (AgentExecution, FileBackedAgentRuntimeRegistry) {
    worker_with_ledger(fixture, creations, client, Arc::new(NoopUsageLedger))
}

/// [`worker_with_client`] whose Agent manager records provider calls in `ledger`.
pub(crate) fn worker_with_ledger(
    fixture: &Fixture,
    creations: model::creation::Creations,
    client: Box<dyn provider_core::ports::agent_session::AgentClient>,
    ledger: Arc<dyn UsageLedger>,
) -> (AgentExecution, FileBackedAgentRuntimeRegistry) {
    crate::testing::spawn(
        fixture.root.path(),
        &fixture.cwd,
        creations,
        client,
        |manager| manager.with_ledger(ledger),
    )
}
