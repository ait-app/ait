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

/// Public script state copied from Paseo's `WorkspaceScriptPayloadSchema`.
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
    /// Loopback proxy URL when a proxy is installed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) local_proxy_url: Option<String>,
    /// Public proxy URL when configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) public_proxy_url: Option<String>,
    /// Backward-compatible preferred proxy URL.
    pub(crate) proxy_url: Option<String>,
    /// Current process lifecycle.
    pub(crate) lifecycle: WorkspaceScriptLifecycle,
    /// Service health; always null because services are not health-probed.
    pub(crate) health: (),
    /// Last exit code.
    pub(crate) exit_code: Option<i32>,
    /// Logical terminal/process identity.
    pub(crate) terminal_id: Option<String>,
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
