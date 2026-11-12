//! Offline Codex app-server fixture shared by this crate's tests and the runtime's execution
//! suite, which drives the fake through `test-support`.

use std::os::unix::fs::symlink;
use std::path::PathBuf;

use domain::agent_runtime::StoredAgentConfig;
use provider_core::ports::agent_session::AgentSessionSpec;
use serde_json::Value;

use crate::CodexClient;

/// Large native Codex history pages for timeline budget tests.
pub mod codex_history;

/// A private copy of the Codex app-server fake with its own data root and working directory.
pub struct Fixture {
    /// Temporary root holding the fake program, its working directory and any native state.
    pub root: tempfile::TempDir,
    /// Path of the fake `codex` program inside `root`.
    pub program: PathBuf,
    /// Working directory of the sessions, where the fake reads behaviors and logs requests.
    pub cwd: PathBuf,
}

impl Fixture {
    /// Link the fake into a new temporary root and create its working directory.
    ///
    /// # Panics
    /// Panics when the temporary root, the link or the working directory cannot be created.
    #[must_use]
    pub fn new() -> Self {
        let root = tempfile::tempdir().expect("create the fixture root");
        let program = root.path().join("codex");
        // Keep the executable immutable while concurrent tests spawn native processes.
        // The link also scopes optional diagnostic cwd overrides to this fixture.
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/codex_app_server.py");
        symlink(source, &program).expect("link the Codex app-server fake");
        let cwd = root.path().join("work");
        std::fs::create_dir(&cwd).expect("create the fixture working directory");
        Self { root, program, cwd }
    }

    /// Session specification for a Codex session in the fixture's working directory.
    ///
    /// # Panics
    /// Panics when the working directory is not valid UTF-8.
    #[must_use]
    pub fn spec(&self) -> AgentSessionSpec {
        AgentSessionSpec {
            provider: "codex".to_owned(),
            cwd: self
                .cwd
                .to_str()
                .expect("fixture working directory is UTF-8")
                .to_owned(),
            config: StoredAgentConfig::default(),
            confinement: None,
        }
    }

    /// A client for the fake program with offline realtime media.
    #[must_use]
    pub fn client(&self) -> CodexClient {
        CodexClient::new(self.program.clone()).with_fixture_media()
    }

    /// Select the fake's scripted `mode` for the following requests.
    ///
    /// # Panics
    /// Panics when the behavior file cannot be written.
    pub fn mode(&self, mode: &str) {
        std::fs::write(self.cwd.join("behavior"), mode).expect("write the fixture behavior");
    }

    /// Every JSON-RPC request the fake has received so far, in arrival order.
    ///
    /// # Panics
    /// Panics when the request log is missing or holds a line that is not JSON.
    #[must_use]
    pub fn requests(&self) -> Vec<Value> {
        std::fs::read_to_string(self.cwd.join("native-requests.jsonl"))
            .expect("read the fixture request log")
            .lines()
            .map(|line| serde_json::from_str(line).expect("fixture request is JSON"))
            .collect()
    }
}

impl Default for Fixture {
    fn default() -> Self {
        Self::new()
    }
}
