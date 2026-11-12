//! Runtime entry points for the tests of adapter crates and `provider-builtin`, compiled only
//! with the `test-support` feature.
//!
//! Production dependents never enable the feature, and the dependency guard rejects non-dev
//! edges that do. Each item delegates to a crate-private runtime operation, so testing an
//! adapter or the built-in assembly from its own crate does not widen the runtime's production
//! API.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use domain::agent_runtime::PersistedAgentRuntimeRecord;
use domain::agent_runtime::registry::AgentRuntimeRegistry;
use domain::workspace::registry::WorkspaceMutationContext;
use model::ErrorCode;
use model::creation::Creations;
use model::events::EventHub;
use model::session::SessionEvents;
use model::workspace::registry::{ProjectRegistry, WorkspaceRegistry};
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
use persistence::storage::registry::{FileBackedProjectRegistry, FileBackedWorkspaceRegistry};
use serde_json::{Value, json};

use crate::composition::Providers;
use crate::protocol::resume::Overrides;
use crate::protocol::timeline::FetchRequest;
use crate::rpc::timeline::fetch_stored;
use crate::service::agent_execution::{AgentExecution, ExecutionDependencies};
use crate::service::agent_manager::{AgentManager, AgentManagerError, AgentRegistration};
use crate::service::agent_runtime::AgentRuntimeDirectory;
use crate::storage::timeline::Timeline;
use provider_core::ports::agent_session::{AgentClient, AgentSessionSpec};
use provider_core::ports::controls::NativeSubagent;
use provider_core::ports::environment::AgentEnvironment;
use provider_core::protocol::agent_config::ConfigPatch;
use provider_core::protocol::prompt::AgentPrompt;
use provider_core::protocol::timeline::NativeItem;

#[cfg(test)]
mod tests;

/// Foreground manager driven directly by a test, without the execution worker.
#[derive(Debug)]
pub struct Manager(AgentManager);

impl Manager {
    /// Drive `manager`; build it with `AgentManager::with_timeline` to persist display history.
    #[must_use]
    pub const fn new(manager: AgentManager) -> Self {
        Self(manager)
    }

    /// Check that `provider` names a registered adapter.
    ///
    /// # Errors
    /// Returns `ProviderUnavailable` when no adapter is registered under `provider`.
    pub fn validate_provider(&self, provider: &str) -> Result<(), AgentManagerError> {
        self.0.validate_provider(provider)
    }

    /// Register one native adapter under its provider identity.
    ///
    /// # Errors
    /// Returns invalid or duplicate provider identities.
    pub fn register(&mut self, client: Box<dyn AgentClient>) -> Result<(), AgentManagerError> {
        self.0.register_client(client)
    }

    /// Create `agent_id` from `spec` with a default, user-visible registration.
    ///
    /// # Errors
    /// Returns invalid or duplicate IDs, unavailable providers, and session or storage failures.
    pub async fn create(
        &mut self,
        agent_id: &str,
        spec: &AgentSessionSpec,
    ) -> Result<PersistedAgentRuntimeRecord, AgentManagerError> {
        self.0
            .create(agent_id, spec, AgentRegistration::default())
            .await
    }

    /// Create `agent_id` from `spec` as an internal session owned by `owner`, the registration a
    /// bot session receives.
    ///
    /// # Errors
    /// Returns invalid or duplicate IDs, unavailable providers, and session or storage failures.
    pub async fn create_internal(
        &mut self,
        agent_id: &str,
        spec: &AgentSessionSpec,
        owner: Value,
    ) -> Result<PersistedAgentRuntimeRecord, AgentManagerError> {
        let registration = AgentRegistration {
            internal: true,
            owner: Some(owner),
            ..AgentRegistration::default()
        };
        self.0.create(agent_id, spec, registration).await
    }

    /// Load the native history of `agent_id` into the display timeline without claiming a
    /// writer, once per agent in this manager.
    ///
    /// # Errors
    /// Returns unknown agents, unavailable history, or durable projection failures.
    pub async fn load_timeline(&mut self, agent_id: &str) -> Result<(), ErrorCode> {
        self.0.load_timeline(agent_id).await
    }

    /// Reopen the persisted native session of `agent_id`.
    ///
    /// # Errors
    /// Returns unknown agents, unavailable providers, and native resume failures.
    pub async fn resume(
        &mut self,
        agent_id: &str,
    ) -> Result<PersistedAgentRuntimeRecord, AgentManagerError> {
        self.0.resume(agent_id).await
    }

    /// Submit one text prompt to `agent_id`.
    ///
    /// # Errors
    /// Returns resume, provider, storage, or busy/read-only failures.
    pub async fn send(&mut self, agent_id: &str, text: &str) -> Result<(), AgentManagerError> {
        self.0.send(agent_id, text).await
    }

    /// Cancel the active turn of `agent_id`.
    ///
    /// # Errors
    /// Returns unknown agents and native cancellation or storage failures.
    pub async fn cancel(&mut self, agent_id: &str) -> Result<(), AgentManagerError> {
        self.0.cancel(agent_id).await
    }

    /// Drain pending native events of every live session once.
    ///
    /// # Errors
    /// Returns storage failures while recording events.
    pub async fn poll(&mut self) -> Result<(), AgentManagerError> {
        self.0.poll().await
    }

    /// Close every live session, reporting the first failure after attempting all of them.
    ///
    /// # Errors
    /// Returns the first native close or storage failure.
    pub async fn close_all(&mut self) -> Result<(), AgentManagerError> {
        self.0.close_all().await
    }

    /// Persist `patch` for the subsequent turns of `agent_id`.
    ///
    /// # Errors
    /// Returns validation, unavailable-provider, archived-agent, or registry errors.
    pub async fn configure(
        &self,
        agent_id: &str,
        patch: &ConfigPatch,
    ) -> Result<(), AgentManagerError> {
        self.0.configure(agent_id, patch).await
    }

    /// Close the live session of `agent_id`, recording its native handle.
    ///
    /// # Errors
    /// Returns native close or registry failures.
    pub async fn close(&mut self, agent_id: &str) -> Result<(), AgentManagerError> {
        self.0.close(agent_id).await
    }

    /// The final assistant text of the latest completed turn of `agent_id`.
    #[must_use]
    pub fn last_message(&self, agent_id: &str) -> Option<&str> {
        self.0.last_message(agent_id)
    }

    /// The active turn of `agent_id`, if one is running.
    #[must_use]
    pub fn active_turn(&self, agent_id: &str) -> Option<&str> {
        self.0.active_turn(agent_id)
    }

    /// The live record of a registered `agent_id`.
    #[must_use]
    pub fn live_snapshot(&self, agent_id: &str) -> Option<&PersistedAgentRuntimeRecord> {
        self.0.live_snapshot(agent_id)
    }

    /// Fill the published `snapshot` of `record` with its capabilities, modes and live controls.
    pub fn control_snapshot(&self, record: &PersistedAgentRuntimeRecord, snapshot: &mut Value) {
        self.0.control_snapshot(record, snapshot);
    }

    /// Create `agent_id` from `spec` with a default registration and private per-process
    /// `environment` overrides that never enter the registry.
    ///
    /// # Errors
    /// Returns identity, admission, native launch or registration failures.
    pub async fn create_with_environment(
        &mut self,
        agent_id: &str,
        spec: &AgentSessionSpec,
        environment: &AgentEnvironment,
    ) -> Result<PersistedAgentRuntimeRecord, AgentManagerError> {
        self.0
            .create_with_environment(agent_id, spec, AgentRegistration::default(), environment)
            .await
    }

    /// Submit one complete rich `prompt` to `agent_id`.
    ///
    /// # Errors
    /// Returns invalid input, busy ownership, native admission or persistence failures.
    pub async fn send_input(
        &mut self,
        agent_id: &str,
        prompt: &AgentPrompt,
    ) -> Result<(), AgentManagerError> {
        self.0.send_input(agent_id, prompt).await
    }

    /// Restore the persisted `agent_id` without overriding any of its stored settings.
    ///
    /// # Errors
    /// Returns missing identity, busy input, invalid settings, provider or persistence failures.
    pub async fn restore(
        &mut self,
        agent_id: &str,
    ) -> Result<PersistedAgentRuntimeRecord, AgentManagerError> {
        self.0.restore(agent_id, &Overrides::default()).await
    }

    /// Execute the provider catalog request `method` with `params` over the registered adapters.
    ///
    /// # Errors
    /// Returns invalid requests, unavailable adapters or discovery and cache errors.
    pub async fn providers(&mut self, method: &str, params: Value) -> Result<Value, ErrorCode> {
        self.0.providers(method, params).await
    }

    /// Read account or host quotas of `provider_id`, scoped to the live `agent_id` when given;
    /// `force` refreshes the source instead of reusing the cache.
    ///
    /// # Errors
    /// Returns unknown identity, invalid scope, registry failure or an oversized projection.
    pub async fn usage(
        &self,
        agent_id: Option<&str>,
        provider_id: Option<&str>,
        force: bool,
    ) -> Result<Value, ErrorCode> {
        self.0.usage(agent_id, provider_id, force).await
    }

    /// The connection event service that publishes the manager's activity notifications.
    #[must_use]
    pub fn events(&self) -> SessionEvents {
        self.0.events()
    }
}

/// Temporary data root and working directory of one execution test.
#[derive(Debug)]
pub struct Workbench {
    /// Data root holding the file-backed registries and the display timeline.
    pub root: tempfile::TempDir,
    /// Existing working directory, registered as the only project and workspace.
    pub cwd: PathBuf,
}

impl Workbench {
    /// Create an empty data root with a `work` directory inside it.
    ///
    /// # Panics
    /// Panics when the temporary directories cannot be created.
    #[must_use]
    pub fn new() -> Self {
        let root = tempfile::tempdir().expect("create the temporary data root");
        let cwd = root.path().join("work");
        std::fs::create_dir(&cwd).expect("create the working directory");
        Self { root, cwd }
    }
}

impl Default for Workbench {
    fn default() -> Self {
        Self::new()
    }
}

/// Start an execution worker over file-backed registries in `bench`, with `client` registered.
///
/// `creations` is shared with the caller to observe agent creation. Returns the execution
/// handle and its durable agent registry.
///
/// # Panics
/// Panics when the registries, the timeline or the worker cannot be set up.
#[must_use]
pub fn worker(
    bench: &Workbench,
    creations: Creations,
    client: Box<dyn AgentClient>,
) -> (AgentExecution, FileBackedAgentRuntimeRegistry) {
    spawn(
        bench.root.path(),
        &bench.cwd,
        creations,
        client,
        |manager| manager,
    )
}

/// Start an execution worker with registries and timeline under `root` and `cwd` registered
/// as the only project and workspace; `configure` adjusts the manager before `client` is
/// registered. Panics on setup errors.
pub(crate) fn spawn(
    root: &Path,
    cwd: &Path,
    creations: Creations,
    client: Box<dyn AgentClient>,
    configure: impl FnOnce(AgentManager) -> AgentManager,
) -> (AgentExecution, FileBackedAgentRuntimeRegistry) {
    let registry = FileBackedAgentRuntimeRegistry::new(root.join("agents.json"));
    registry
        .initialize()
        .expect("initialize the agent registry");
    let projects = FileBackedProjectRegistry::new(root.join("projects.json"));
    projects
        .initialize()
        .expect("initialize the project registry");
    let workspaces = FileBackedWorkspaceRegistry::new(root.join("workspaces.json"));
    workspaces
        .initialize()
        .expect("initialize the workspace registry");
    projects.upsert(&serde_json::from_value(json!({
        "projectId":"project-1","rootPath":cwd,"kind":"non_git","displayName":"project",
        "createdAt":"2026-09-24T00:00:00Z","updatedAt":"2026-09-24T00:00:00Z","archivedAt":null
    })).expect("decode the test project")).expect("register the test project");
    workspaces
        .upsert(
            &serde_json::from_value(json!({
                "workspaceId":"wks_0123456789abcdef","projectId":"project-1","cwd":cwd,
                "kind":"local_checkout","displayName":"main","createdAt":"2026-09-24T00:00:00Z",
                "updatedAt":"2026-09-24T00:00:00Z","archivedAt":null
            }))
            .expect("decode the test workspace"),
            WorkspaceMutationContext::default(),
        )
        .expect("register the test workspace");
    let mut manager = configure(
        AgentManager::new(Box::new(registry.clone()))
            .with_creations(creations)
            .with_timeline(
                Timeline::open(&root.join("timeline.sqlite")).expect("open the display timeline"),
            ),
    );
    manager
        .register_client(client)
        .expect("register the adapter under test");
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
    .expect("start the execution worker");
    (worker, registry)
}

/// Execute `method` with `params` on `execution` and return its result.
///
/// # Panics
/// Fails the test on an error result or when no result arrives within 30 seconds.
pub async fn request(execution: &AgentExecution, method: &str, params: Value) -> Value {
    tokio::time::timeout(Duration::from_secs(30), execution.execute(method, params))
        .await
        .unwrap_or_else(|_| panic!("{method} timed out"))
        .unwrap_or_else(|error| panic!("{method}: {error:?}"))
}

/// Largest serialized size, in bytes, of one display timeline entry the runtime stores.
pub const MAX_TIMELINE_ENTRY_BYTES: usize = crate::storage::timeline::MAX_ENTRY_BYTES;

/// Fetch one page of the stored display history in `scope`, as `agent.timeline.get.request`
/// answers it.
///
/// `request` holds the request parameters, `agent` the agent metadata of the response envelope
/// and `history_needs_refresh` the optional refresh flag reserved in the response budget.
///
/// # Errors
/// Returns `InvalidMessage` for undecodable parameters, and storage, cursor validation or
/// response-budget errors.
pub fn fetch_timeline(
    timeline: &Timeline,
    scope: &str,
    request: Value,
    agent: &Value,
    history_needs_refresh: Option<bool>,
) -> Result<Value, ErrorCode> {
    let request: FetchRequest =
        serde_json::from_value(request).map_err(|_| ErrorCode::InvalidMessage)?;
    fetch_stored(&request, timeline, scope, agent, history_needs_refresh)
}

/// Adapters in `providers`, in assembly order.
#[must_use]
pub fn clients(providers: &Providers) -> &[Box<dyn AgentClient>] {
    &providers.clients
}

/// The adapters of `providers` that implement structured auxiliary generation.
#[must_use]
pub fn summary_clients(providers: &Providers) -> &[Arc<dyn AgentClient>] {
    &providers.summary_clients
}

/// Open an empty in-memory display timeline.
///
/// # Errors
/// Returns schema initialization failures.
pub fn memory_timeline() -> Result<Timeline, ErrorCode> {
    Timeline::memory()
}

/// The change notifications published by `timeline`.
#[must_use]
pub fn timeline_events(timeline: &Timeline) -> EventHub {
    timeline.events()
}

/// The generation and ordered display entries of `agent`.
///
/// # Errors
/// Returns storage or invalid persisted-data errors.
pub fn timeline_entries(
    timeline: &Timeline,
    agent: &str,
) -> Result<(String, Vec<NativeItem>), ErrorCode> {
    let (epoch, rows) = timeline.read(agent)?;
    Ok((epoch, rows.into_iter().map(|row| row.entry).collect()))
}

/// Append `entries` produced by `provider` to the display history of `agent`.
///
/// # Errors
/// Returns storage or invalid-entry errors.
pub fn append_timeline(
    timeline: &Timeline,
    agent: &str,
    provider: &str,
    entries: &[NativeItem],
) -> Result<(String, Vec<u64>), ErrorCode> {
    timeline.append(agent, provider, entries)
}

/// Persist and publish one incremental `entry` of the native `observation` produced by
/// `provider` for `agent`; text entries are additive deltas, tool entries running snapshots.
///
/// # Errors
/// Returns changed replays, already completed items, oversized data or storage failures.
pub fn progress_timeline(
    timeline: &Timeline,
    agent: &str,
    provider: &str,
    observation: &str,
    entry: &NativeItem,
) -> Result<(), ErrorCode> {
    timeline.progress(agent, provider, observation, entry)
}

/// Replace the display history of `agent` with `entries`, returning the new generation.
///
/// # Errors
/// Returns storage or invalid-entry errors.
pub fn reconcile_timeline(
    timeline: &Timeline,
    agent: &str,
    provider: &str,
    entries: &[NativeItem],
) -> Result<String, ErrorCode> {
    timeline.reconcile(agent, provider, entries)
}

/// The stored native subagent descriptors of `parent`.
///
/// # Errors
/// Returns storage failures or an undecodable stored descriptor.
pub fn timeline_subagents(
    timeline: &Timeline,
    parent: &str,
) -> Result<Vec<NativeSubagent>, ErrorCode> {
    timeline.subagents(parent)
}
