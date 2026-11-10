//! Wire request and response schemas.
//!
//! Workspace setup and script RPC payloads copied from Paseo's public schemas.

use serde::{Deserialize, Serialize};

/// Select one workspace.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceSetupRequest {
    /// Durable workspace identity.
    pub(crate) workspace_id: String,
}

/// Select one configured workspace script.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceScriptRequest {
    /// Durable workspace identity.
    pub(crate) workspace_id: String,
    /// Exact key under `scripts` in `ait.json`.
    pub(crate) script_name: String,
}

/// Start one configured workspace script.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceScriptStartRequest {
    /// Durable workspace identity.
    pub(crate) workspace_id: String,
    /// Exact key under `scripts` in `ait.json`.
    pub(crate) script_name: String,
    /// Positional parameters for scripts that declare `acceptsArgs`.
    #[serde(default)]
    pub(crate) args: Vec<String>,
}

/// Select one retained script run by identity, or the latest run of a script.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceScriptRunRequest {
    /// Durable workspace identity.
    pub(crate) workspace_id: String,
    /// Run identity; takes precedence over `script_name`.
    #[serde(default)]
    pub(crate) run_id: Option<String>,
    /// Script whose latest run is read when `run_id` is absent.
    #[serde(default)]
    pub(crate) script_name: Option<String>,
}

/// Wait for one script run to finish.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceScriptWaitRequest {
    /// Durable workspace identity.
    pub(crate) workspace_id: String,
    /// Run identity returned by `workspace.script.start.request`.
    pub(crate) run_id: String,
    /// Wait bound in milliseconds; a timeout does not stop the run.
    #[serde(default)]
    pub(crate) timeout_ms: Option<u64>,
}

/// State of one setup command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkspaceSetupCommandStatus {
    /// The command has started but has not exited.
    Running,
    /// The command exited successfully.
    Completed,
    /// The command exited unsuccessfully.
    Failed,
}

/// Snapshot of one command in a workspace setup run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceSetupCommand {
    /// One-based command position.
    pub(crate) index: usize,
    /// Shell command from `ait.json`.
    pub(crate) command: String,
    /// Directory in which the command runs.
    pub(crate) cwd: String,
    /// Bounded combined output.
    pub(crate) log: String,
    /// Current command state.
    pub(crate) status: WorkspaceSetupCommandStatus,
    /// Process exit code, or null while running or when terminated by a signal.
    pub(crate) exit_code: Option<i32>,
    /// Elapsed milliseconds after completion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) duration_ms: Option<u64>,
}

/// Paseo worktree setup detail payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceSetupDetail {
    /// Fixed Paseo detail discriminator.
    #[serde(rename = "type")]
    pub(crate) kind: String,
    /// Backing worktree or directory path.
    pub(crate) worktree_path: String,
    /// Git branch when known.
    pub(crate) branch_name: String,
    /// Rendered bounded setup transcript.
    pub(crate) log: String,
    /// Per-command snapshots.
    pub(crate) commands: Vec<WorkspaceSetupCommand>,
    /// Present only when output was truncated.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) truncated: bool,
}

/// Overall setup lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkspaceSetupStatus {
    /// Setup commands are running.
    Running,
    /// Every setup command completed.
    Completed,
    /// A setup command failed.
    Failed,
    /// Automation awaits explicit trust.
    Blocked,
}

/// Persisted untrusted checkout provenance exposed with a blocked snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum WorkspaceBlockedSource {
    /// A change request from another repository.
    ChangeRequest {
        /// Forge identifier.
        forge: String,
        /// Positive change-request number.
        number: u64,
        /// Repository that supplied the head branch.
        head_repository: String,
    },
}

/// Cached setup status returned by polling and progress events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSetupSnapshot {
    /// Monotonic in-memory revision within this daemon process.
    pub(crate) revision: u64,
    /// Overall setup lifecycle.
    pub(crate) status: WorkspaceSetupStatus,
    /// Worktree setup transcript.
    pub(crate) detail: WorkspaceSetupDetail,
    /// Safe failure text.
    pub(crate) error: Option<String>,
    /// Present while automation is blocked for untrusted code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) blocked_source: Option<WorkspaceBlockedSource>,
}

/// Setup status polling result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceSetupStatusResult {
    /// Requested workspace identity.
    pub(crate) workspace_id: String,
    /// Last in-memory snapshot or a derived blocked snapshot.
    pub(crate) snapshot: Option<WorkspaceSetupSnapshot>,
}

/// Explicit setup approval/start result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceSetupRunResult {
    /// Requested workspace identity.
    pub(crate) workspace_id: String,
    /// Whether an automation block was cleared and a setup run started.
    pub(crate) started: bool,
    /// Safe failure text.
    pub(crate) error: Option<String>,
}

/// Script classification from `ait.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkspaceScriptType {
    /// One-shot shell command.
    Script,
    /// Long-running service with an optional TCP port.
    Service,
}

/// Script process lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkspaceScriptLifecycle {
    /// The child process has not exited.
    Running,
    /// The child process is absent or has exited.
    Stopped,
}

/// Public script state used by Workspace RPCs and directory events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceScript {
    /// Exact configuration key.
    pub(crate) script_name: String,
    /// Plain script or service.
    #[serde(rename = "type")]
    pub(crate) kind: WorkspaceScriptType,
    /// Stable service hostname; plain scripts use their script name.
    pub(crate) hostname: String,
    /// Configured or allocated service port.
    pub(crate) port: Option<u16>,
    /// Current process lifecycle.
    pub(crate) lifecycle: WorkspaceScriptLifecycle,
    /// Service health; always null because services are not health-probed.
    pub(crate) health: (),
    /// Last exit code.
    pub(crate) exit_code: Option<i32>,
    /// Logical terminal/process identity.
    pub(crate) terminal_id: Option<String>,
    /// Most recent run, readable through `workspace.script.run.get.request`.
    pub(crate) run_id: Option<String>,
    /// Whether callers may pass arguments.
    pub(crate) accepts_args: bool,
}

/// Lifecycle and result of one script run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkspaceScriptRunOutcome {
    /// The child process has not exited.
    Running,
    /// The child exited with status zero.
    Succeeded,
    /// The child exited unsuccessfully or was killed outside Ait.
    Failed,
    /// Ait stopped the child.
    Stopped,
}

/// One recorded script run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceScriptRun {
    /// Daemon-assigned run identity.
    pub(crate) run_id: String,
    /// Exact configuration key.
    pub(crate) script_name: String,
    /// Caller arguments.
    pub(crate) args: Vec<String>,
    /// `HEAD` commit at start, when the Workspace is a Git checkout.
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
    pub(crate) outcome: WorkspaceScriptRunOutcome,
    /// Exit code, or null while running or after a signal.
    pub(crate) exit_code: Option<i32>,
    /// Bounded combined output; null when a wait timed out before completion.
    pub(crate) output: Option<String>,
    /// Total bytes written by the run.
    pub(crate) output_bytes: u64,
    /// Whether the retained output omits part of the written bytes.
    pub(crate) truncated: bool,
}

/// Script run read or wait result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceScriptRunResult {
    /// Requested workspace identity.
    pub(crate) workspace_id: String,
    /// Selected run.
    pub(crate) run: Option<WorkspaceScriptRun>,
    /// Present on wait results: whether the bound elapsed before the run finished.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) timed_out: Option<bool>,
    /// Safe failure text.
    pub(crate) error: Option<String>,
}

/// Script list result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceScriptListResult {
    /// Requested workspace identity.
    pub(crate) workspace_id: String,
    /// Configured scripts plus running orphan entries.
    pub(crate) scripts: Vec<WorkspaceScript>,
    /// Safe failure text.
    pub(crate) error: Option<String>,
}

/// Script start/stop result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceScriptMutationResult {
    /// Requested workspace identity.
    pub(crate) workspace_id: String,
    /// Requested script key.
    pub(crate) script_name: String,
    /// Updated script state.
    pub(crate) script: Option<WorkspaceScript>,
    /// Safe failure text.
    pub(crate) error: Option<String>,
}

#[cfg(test)]
mod tests;
