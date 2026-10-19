//! Transport-independent request decoding and projection.
//!
//! Transport-independent workspace automation request handling.

/// Client methods implemented by this component; consumed by capability discovery.
pub const METHODS: &[MethodSpec] = &[
    MethodSpec::request("workspace.setup.status.request"),
    MethodSpec::request("workspace.setup.summary.request"),
    MethodSpec::request("workspace.setup.subscribe.request"),
    MethodSpec::request("workspace.setup.run.request"),
    MethodSpec::request("workspace.script.list.request"),
    MethodSpec::request("workspace.script.start.request"),
    MethodSpec::request("workspace.script.stop.request"),
];

use domain::workspace::records::UntrustedWorkspaceSource;
use model::methods::MethodSpec;
use serde_json::Value;

use crate::automation::ports::{ScriptSnapshot, ScriptType, SetupLifecycle, SetupSnapshot};
use crate::automation::protocol::{
    WorkspaceBlockedSource, WorkspaceScript, WorkspaceScriptLifecycle, WorkspaceScriptListResult,
    WorkspaceScriptMutationResult, WorkspaceScriptRequest, WorkspaceScriptType,
    WorkspaceSetupCommand, WorkspaceSetupCommandStatus, WorkspaceSetupDetail,
    WorkspaceSetupRequest, WorkspaceSetupRunResult, WorkspaceSetupSnapshot, WorkspaceSetupStatus,
    WorkspaceSetupStatusResult,
};
use crate::automation::service::{SetupStatus, WorkspaceAutomation};
use crate::support::error::ErrorCode;
use crate::support::rpc::{decode, encode};
use crate::support::time::now_timestamp;

/// Decode and execute a business request.
///
/// # Errors
/// Returns stable business failures for invalid or unsuccessful requests.
pub(crate) fn execute(
    automation: &WorkspaceAutomation,
    method: &str,
    params: Value,
) -> Result<Value, ErrorCode> {
    match method {
        "workspace.setup.status.request" => setup_status(automation, decode(params)?),
        "workspace.setup.summary.request" => {
            let result = setup_status(automation, decode(params)?)?;
            Ok(
                serde_json::json!({"workspaceId":result["workspaceId"], "summary":
                result["snapshot"].is_object().then(|| setup_summary(&result["snapshot"]))}),
            )
        }
        "workspace.setup.run.request" => setup_run(automation, decode(params)?),
        "workspace.script.list.request" => list_scripts(automation, decode(params)?),
        "workspace.script.start.request" => start_script(automation, &decode(params)?),
        "workspace.script.stop.request" => stop_script(automation, &decode(params)?),
        _ => Err(ErrorCode::MethodNotFound),
    }
}

fn setup_status(
    automation: &WorkspaceAutomation,
    request: WorkspaceSetupRequest,
) -> Result<Value, ErrorCode> {
    let snapshot = match automation.setup_status(&request.workspace_id) {
        Ok(SetupStatus::Absent) => None,
        Ok(SetupStatus::Snapshot(snapshot)) => Some(setup_snapshot(snapshot, None)),
        Ok(SetupStatus::Blocked { placement, source }) => Some(WorkspaceSetupSnapshot {
            revision: 0,
            status: WorkspaceSetupStatus::Blocked,
            detail: WorkspaceSetupDetail {
                kind: "worktree_setup".to_owned(),
                worktree_path: placement.worktree_path,
                branch_name: placement.branch_name,
                log: String::new(),
                commands: Vec::new(),
                truncated: false,
            },
            error: None,
            blocked_source: Some(blocked_source(source)),
        }),
        Err(error) => {
            return encode(
                WorkspaceSetupStatusResult {
                    workspace_id: request.workspace_id,
                    snapshot: Some(WorkspaceSetupSnapshot {
                        revision: 0,
                        status: WorkspaceSetupStatus::Failed,
                        detail: WorkspaceSetupDetail {
                            kind: "worktree_setup".to_owned(),
                            worktree_path: String::new(),
                            branch_name: String::new(),
                            log: String::new(),
                            commands: Vec::new(),
                            truncated: false,
                        },
                        error: Some(error.to_string()),
                        blocked_source: None,
                    }),
                },
                ErrorCode::RegistryIo,
            );
        }
    };
    encode(
        WorkspaceSetupStatusResult {
            workspace_id: request.workspace_id,
            snapshot,
        },
        ErrorCode::RegistryIo,
    )
}

/// Project setup status without command text or logs for global observers.
#[must_use]
pub fn setup_summary(snapshot: &Value) -> Value {
    serde_json::json!({"status":snapshot["status"], "revision":snapshot["revision"],
        "hasCommands":snapshot["detail"]["commands"].as_array().is_some_and(|commands| !commands.is_empty())})
}

fn setup_run(
    automation: &WorkspaceAutomation,
    request: WorkspaceSetupRequest,
) -> Result<Value, ErrorCode> {
    let result = automation.approve_and_start_setup(&request.workspace_id, &now_timestamp());
    let (started, error) = match result {
        Ok(started) => (started, None),
        Err(error) => (false, Some(error.to_string())),
    };
    encode(
        WorkspaceSetupRunResult {
            workspace_id: request.workspace_id,
            started,
            error,
        },
        ErrorCode::RegistryIo,
    )
}

fn list_scripts(
    automation: &WorkspaceAutomation,
    request: WorkspaceSetupRequest,
) -> Result<Value, ErrorCode> {
    let result = automation.list_scripts(&request.workspace_id);
    let (scripts, error) = match result {
        Ok(scripts) => (scripts.into_iter().map(script).collect(), None),
        Err(error) => (Vec::new(), Some(error.to_string())),
    };
    encode(
        WorkspaceScriptListResult {
            workspace_id: request.workspace_id,
            scripts,
            error,
        },
        ErrorCode::RegistryIo,
    )
}

fn start_script(
    automation: &WorkspaceAutomation,
    request: &WorkspaceScriptRequest,
) -> Result<Value, ErrorCode> {
    script_mutation(
        request,
        automation.start_script(&request.workspace_id, &request.script_name),
    )
}

fn stop_script(
    automation: &WorkspaceAutomation,
    request: &WorkspaceScriptRequest,
) -> Result<Value, ErrorCode> {
    script_mutation(
        request,
        automation.stop_script(&request.workspace_id, &request.script_name),
    )
}

fn script_mutation(
    request: &WorkspaceScriptRequest,
    result: Result<ScriptSnapshot, crate::automation::service::WorkspaceAutomationServiceError>,
) -> Result<Value, ErrorCode> {
    let (script, error) = match result {
        Ok(snapshot) => (Some(script(snapshot)), None),
        Err(error) => (None, Some(error.to_string())),
    };
    encode(
        WorkspaceScriptMutationResult {
            workspace_id: request.workspace_id.clone(),
            script_name: request.script_name.clone(),
            script,
            error,
        },
        ErrorCode::RegistryIo,
    )
}

/// Project a runtime setup snapshot into the wire shape used by status and progress.
#[must_use]
pub fn setup_snapshot(
    snapshot: SetupSnapshot,
    blocked_source: Option<WorkspaceBlockedSource>,
) -> WorkspaceSetupSnapshot {
    WorkspaceSetupSnapshot {
        revision: snapshot.revision,
        status: match snapshot.lifecycle {
            SetupLifecycle::Running => WorkspaceSetupStatus::Running,
            SetupLifecycle::Completed => WorkspaceSetupStatus::Completed,
            SetupLifecycle::Failed => WorkspaceSetupStatus::Failed,
        },
        detail: WorkspaceSetupDetail {
            kind: "worktree_setup".to_owned(),
            worktree_path: snapshot.worktree_path,
            branch_name: snapshot.branch_name,
            log: snapshot.log,
            commands: snapshot
                .commands
                .into_iter()
                .map(|command| WorkspaceSetupCommand {
                    index: command.index,
                    command: command.command,
                    cwd: command.cwd,
                    log: command.log,
                    status: if command.running {
                        WorkspaceSetupCommandStatus::Running
                    } else if command.exit_code == Some(0) {
                        WorkspaceSetupCommandStatus::Completed
                    } else {
                        WorkspaceSetupCommandStatus::Failed
                    },
                    exit_code: command.exit_code,
                    duration_ms: command.duration_ms,
                })
                .collect(),
            truncated: snapshot.truncated,
        },
        error: snapshot.error,
        blocked_source,
    }
}

fn blocked_source(source: UntrustedWorkspaceSource) -> WorkspaceBlockedSource {
    match source {
        UntrustedWorkspaceSource::ChangeRequest {
            forge,
            number,
            head_repository,
        } => WorkspaceBlockedSource::ChangeRequest {
            forge,
            number,
            head_repository,
        },
    }
}

/// Project a runtime script snapshot into the wire shape used by RPCs and events.
#[must_use]
pub fn script(snapshot: ScriptSnapshot) -> WorkspaceScript {
    WorkspaceScript {
        script_name: snapshot.name,
        kind: match snapshot.kind {
            ScriptType::Script => WorkspaceScriptType::Script,
            ScriptType::Service => WorkspaceScriptType::Service,
        },
        hostname: snapshot.hostname,
        port: snapshot.port,
        lifecycle: if snapshot.running {
            WorkspaceScriptLifecycle::Running
        } else {
            WorkspaceScriptLifecycle::Stopped
        },
        health: (),
        exit_code: snapshot.exit_code,
        terminal_id: snapshot.terminal_id,
    }
}

#[cfg(test)]
mod tests;
