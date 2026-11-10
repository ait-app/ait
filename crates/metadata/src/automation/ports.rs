//! Blocking adapter boundaries.
//!
//! Blocking process boundary for workspace setup and `ait.json` scripts.

use std::fmt::Debug;
use std::sync::Arc;

/// Committed setup or script change sent toward interested clients.
#[derive(Debug, Clone)]
pub enum AutomationEvent {
    /// Complete script snapshot after a lifecycle transition.
    Scripts {
        /// Workspace identity.
        workspace_id: String,
        /// Current configured and active scripts.
        scripts: Vec<ScriptSnapshot>,
    },
    /// In-memory setup progress after a transition.
    Setup {
        /// Workspace identity.
        workspace_id: String,
        /// Current setup details.
        snapshot: SetupSnapshot,
    },
}

/// Callback installed by the host; event publication cannot fail a committed mutation.
pub(crate) type AutomationEventSink = Arc<dyn Fn(AutomationEvent) + Send + Sync>;

/// Configured script type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptType {
    /// One-shot shell command.
    Script,
    /// Long-running service.
    Service,
}

/// One configured script and its current runtime projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptSnapshot {
    /// Exact `ait.json` key.
    pub(crate) name: String,
    /// Plain script or service.
    pub(crate) kind: ScriptType,
    /// Stable hostname projection.
    pub(crate) hostname: String,
    /// Configured service port.
    pub(crate) port: Option<u16>,
    /// Whether the child is still running.
    pub(crate) running: bool,
    /// Last process exit code.
    pub(crate) exit_code: Option<i32>,
    /// Logical child identity.
    pub(crate) terminal_id: Option<String>,
    /// Most recent recorded run of this script.
    pub(crate) run_id: Option<String>,
    /// Whether `ait.json` lets callers pass arguments to this script.
    pub(crate) accepts_args: bool,
}

/// Lifecycle and result of one script run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptRunOutcome {
    /// The child process has not exited.
    Running,
    /// The child exited with status zero.
    Succeeded,
    /// The child exited unsuccessfully or was killed outside Ait.
    Failed,
    /// Ait stopped the child on request or during Workspace retirement.
    Stopped,
}

/// One recorded script run with its evidence and bounded combined output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptRunSnapshot {
    /// Daemon-assigned run identity.
    pub(crate) run_id: String,
    /// Exact `ait.json` key.
    pub(crate) script_name: String,
    /// Caller arguments passed as positional parameters.
    pub(crate) args: Vec<String>,
    /// `HEAD` commit of the Workspace checkout at start, when it is a Git checkout.
    pub(crate) commit: Option<String>,
    /// Whether the checkout had uncommitted or untracked changes at start, when known.
    pub(crate) dirty: Option<bool>,
    /// RFC 3339 start time.
    pub(crate) started_at: String,
    /// RFC 3339 completion time.
    pub(crate) finished_at: Option<String>,
    /// Elapsed milliseconds after completion.
    pub(crate) duration_ms: Option<u64>,
    /// Current lifecycle or final result.
    pub(crate) outcome: ScriptRunOutcome,
    /// Exit code, or none while running or after a signal.
    pub(crate) exit_code: Option<i32>,
    /// Bounded stdout and stderr, interleaved in write order.
    pub(crate) output: String,
    /// Total bytes the run has written, including bytes dropped from `output`.
    pub(crate) output_bytes: u64,
    /// Whether `output` omits part of the written bytes.
    pub(crate) truncated: bool,
}

/// Selects one recorded script run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptRunTarget<'a> {
    /// The run with this identity.
    Run(&'a str),
    /// The most recent run of the named script.
    LatestOf(&'a str),
}

/// Resolves once a script run has finished or its record has been discarded.
#[derive(Debug, Clone)]
pub struct ScriptRunCompletion(tokio::sync::watch::Receiver<bool>);

impl ScriptRunCompletion {
    /// Observe a run whose sender publishes `true` when the run finishes.
    pub(crate) const fn new(receiver: tokio::sync::watch::Receiver<bool>) -> Self {
        Self(receiver)
    }

    /// Wait until the run finishes; a dropped record also ends the wait.
    pub(crate) async fn finished(mut self) {
        let _ = self.0.wait_for(|finished| *finished).await;
    }
}

/// Teardown commands prepared for one checkout, run without holding service locks.
pub struct TeardownJob(Box<dyn FnOnce() -> Result<(), WorkspaceAutomationError> + Send>);

impl TeardownJob {
    /// Wrap the blocking command sequence of one teardown.
    pub(crate) fn new(
        run: impl FnOnce() -> Result<(), WorkspaceAutomationError> + Send + 'static,
    ) -> Self {
        Self(Box::new(run))
    }

    /// Run every teardown command in order, stopping at the first failure.
    ///
    /// # Errors
    /// Returns the first failed, timed-out, oversized or unstartable command.
    pub fn run(self) -> Result<(), WorkspaceAutomationError> {
        (self.0)()
    }
}

impl Debug for TeardownJob {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TeardownJob")
            .finish_non_exhaustive()
    }
}

/// One setup command snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupCommandSnapshot {
    /// One-based command position.
    pub(crate) index: usize,
    /// Shell command.
    pub(crate) command: String,
    /// Execution directory.
    pub(crate) cwd: String,
    /// Bounded combined output.
    pub(crate) log: String,
    /// Whether the command is still running.
    pub(crate) running: bool,
    /// Exit code after completion.
    pub(crate) exit_code: Option<i32>,
    /// Elapsed milliseconds after completion.
    pub(crate) duration_ms: Option<u64>,
}

/// Setup lifecycle independent of the wire schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupLifecycle {
    /// Commands are running.
    Running,
    /// Every command completed.
    Completed,
    /// A command failed or setup could not start.
    Failed,
}

/// Current setup snapshot retained in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupSnapshot {
    /// Monotonic in-memory revision for bootstrap/event ordering.
    pub revision: u64,
    /// Overall lifecycle.
    pub lifecycle: SetupLifecycle,
    /// Backing worktree or directory.
    pub worktree_path: String,
    /// Git branch when known.
    pub branch_name: String,
    /// Rendered bounded setup transcript.
    pub log: String,
    /// Per-command snapshots.
    pub commands: Vec<SetupCommandSnapshot>,
    /// Whether output was truncated.
    pub truncated: bool,
    /// Safe error text.
    pub error: Option<String>,
}

/// Workspace placement supplied to the runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePlacement {
    /// Durable workspace identity.
    pub(crate) workspace_id: String,
    /// Directory containing `ait.json` and used as command cwd.
    pub(crate) cwd: String,
    /// Backing worktree root.
    pub(crate) worktree_path: String,
    /// Main checkout root.
    pub(crate) repo_root: String,
    /// Current branch.
    pub(crate) branch_name: String,
}

/// Stable workspace automation failures.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorkspaceAutomationError {
    /// `ait.json` is malformed, unsafe, or too large.
    #[error("{0}")]
    InvalidConfig(String),
    /// The named script is absent or malformed.
    #[error("Script '{0}' is not configured in ait.json")]
    UnknownScript(String),
    /// A process with the same workspace/script key is already running.
    #[error("Script '{0}' is already running")]
    AlreadyRunning(String),
    /// No running process can be stopped.
    #[error("Script '{0}' is not running")]
    NotRunning(String),
    /// No retained run matches the request.
    #[error("Script run '{0}' was not found")]
    UnknownRun(String),
    /// Caller arguments are refused for this script or exceed the bounds.
    #[error("{0}")]
    InvalidArguments(String),
    /// Process or filesystem work failed.
    #[error("{0}")]
    Io(String),
}

/// Runtime for background setup and configured scripts.
pub trait WorkspaceAutomationRuntime: Debug + Send + Sync {
    /// Install a callback for background setup transitions.
    fn set_event_sink(&mut self, _sink: AutomationEventSink) {}
    /// Stop selected Workspace setup and script processes, retaining failed cleanup for retry.
    /// # Errors
    /// Returns termination, reaping, or bounded cancellation timeout failures.
    fn close_workspaces(&self, workspace_ids: &[String]) -> Result<(), WorkspaceAutomationError>;

    /// List configured scripts and refresh child exit state.
    ///
    /// # Errors
    /// Returns configuration, filesystem, or process inspection failures.
    fn list_scripts(
        &self,
        workspace: &WorkspacePlacement,
    ) -> Result<Vec<ScriptSnapshot>, WorkspaceAutomationError>;

    /// Start one configured script, recording a new run.
    ///
    /// `args` reach the shell as positional parameters and are never parsed as shell code.
    ///
    /// # Errors
    /// Returns configuration, refused-argument, duplicate-run, or spawn failures.
    fn start_script(
        &self,
        workspace: &WorkspacePlacement,
        script_name: &str,
        args: &[String],
    ) -> Result<ScriptSnapshot, WorkspaceAutomationError>;

    /// Read one retained run with refreshed process state.
    ///
    /// # Errors
    /// Returns `UnknownRun` when no retained run matches, or process inspection failures.
    fn script_run(
        &self,
        workspace_id: &str,
        target: ScriptRunTarget<'_>,
    ) -> Result<ScriptRunSnapshot, WorkspaceAutomationError>;

    /// Observe the completion of one retained run.
    ///
    /// # Errors
    /// Returns `UnknownRun` when no retained run matches.
    fn script_run_completion(
        &self,
        workspace_id: &str,
        run_id: &str,
    ) -> Result<ScriptRunCompletion, WorkspaceAutomationError>;

    /// Prepare the configured teardown commands for a checkout about to be removed.
    /// Returns none when no teardown is configured.
    ///
    /// # Errors
    /// Returns configuration or port allocation failures.
    fn prepare_teardown(
        &self,
        workspace: &WorkspacePlacement,
    ) -> Result<Option<TeardownJob>, WorkspaceAutomationError>;

    /// Stop one running script.
    ///
    /// # Errors
    /// Returns missing-run or process termination failures.
    fn stop_script(
        &self,
        workspace: &WorkspacePlacement,
        script_name: &str,
    ) -> Result<ScriptSnapshot, WorkspaceAutomationError>;

    /// Start setup commands in the background. Returns false when a run is already active.
    ///
    /// # Errors
    /// Returns configuration or thread/process startup failures.
    fn start_setup(&self, workspace: &WorkspacePlacement)
    -> Result<bool, WorkspaceAutomationError>;

    /// Read the latest in-memory setup snapshot.
    fn setup_snapshot(&self, workspace_id: &str) -> Option<SetupSnapshot>;
}
