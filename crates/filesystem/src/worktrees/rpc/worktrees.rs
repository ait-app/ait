//! Worktree request validation and response projection.

/// Client methods implemented by this component; consumed by capability discovery.
pub const METHODS: &[MethodSpec] = &[
    MethodSpec::request("workspace.worktree.list.request"),
    MethodSpec::request("workspace.worktree.create.request"),
    MethodSpec::request("workspace.worktree.archive.request"),
];

use chrono::{SecondsFormat, Utc};
use model::methods::MethodSpec;
use serde::Serialize;
use serde_json::Value;

use crate::support::error::ErrorCode;
use crate::worktrees::protocol::worktrees::{
    CheckoutError, CheckoutErrorCode, WorktreeArchiveRequest, WorktreeArchiveResult,
    WorktreeArchiveScope, WorktreeCreateAction, WorktreeCreateRequest, WorktreeCreateResult,
    WorktreeListEntry, WorktreeListRequest, WorktreeListResult,
};
use crate::worktrees::service::worktrees::{
    ArchiveScope, ArchiveWorktree, CreateAction, CreateWorktree, WorktreeFailureKind, Worktrees,
    WorktreesError,
};

/// Completed dispatch plus an optional workspace event sent after the response.
pub(crate) struct Dispatched {
    /// Serialized response.
    pub(crate) value: Value,
    /// Workspace update to publish after the response.
    pub(crate) event: Option<Value>,
    /// Workspace whose setup should be scheduled by the host.
    pub(crate) created_workspace_id: Option<String>,
}

/// Execute a filesystem request.
///
/// # Errors
/// Rejects invalid parameters, unknown methods, or failed result encoding.
pub(crate) fn execute(
    worktrees: &mut Worktrees,
    method: &str,
    params: Value,
) -> Result<Dispatched, ErrorCode> {
    match method {
        "workspace.worktree.list.request" => list(worktrees, decode(params)?),
        "workspace.worktree.create.request" => create(worktrees, decode(params)?),
        "workspace.worktree.archive.request" => archive(worktrees, decode(params)?),
        _ => Err(ErrorCode::MethodNotFound),
    }
}

fn list(worktrees: &Worktrees, request: WorktreeListRequest) -> Result<Dispatched, ErrorCode> {
    let Some(cwd) = request.repo_root.or(request.cwd) else {
        return value(WorktreeListResult {
            worktrees: Vec::new(),
            error: Some(CheckoutError {
                code: CheckoutErrorCode::Unknown,
                message: "cwd or repoRoot is required".to_owned(),
            }),
        });
    };
    match worktrees.list(&cwd) {
        Ok(entries) => value(WorktreeListResult {
            worktrees: entries
                .into_iter()
                .map(|entry| WorktreeListEntry {
                    worktree_path: entry.path,
                    created_at: entry.created_at,
                    branch_name: entry.branch_name,
                    head: entry.head,
                })
                .collect(),
            error: None,
        }),
        Err(error) => value(WorktreeListResult {
            worktrees: Vec::new(),
            error: Some(checkout_error(&error)),
        }),
    }
}

fn create(worktrees: &Worktrees, request: WorktreeCreateRequest) -> Result<Dispatched, ErrorCode> {
    let context = request.normalized_first_agent_context();
    let input = CreateWorktree {
        workspace_id: None,
        title: None,
        branch_name: None,
        base_branch: None,
        cwd: request.cwd,
        project_id: request.project_id,
        worktree_slug: request.worktree_slug,
        ref_name: request.ref_name,
        action: match request.action.unwrap_or(WorktreeCreateAction::BranchOff) {
            WorktreeCreateAction::BranchOff => CreateAction::BranchOff,
            WorktreeCreateAction::Checkout => CreateAction::Checkout,
        },
        checkout_source: request
            .checkout_source
            .map(domain::workspace::protocol::worktree_source::ChangeRequestCheckoutSource::into_intent)
            .or_else(|| {
                request.github_pr_number.map(|number| {
                    domain::workspace::worktrees::WorktreeChangeRequest {
                        forge: Some("github".to_owned()),
                        number,
                        project_path: None,
                    }
                })
            }),
        first_agent_prompt: context.as_ref().and_then(|context| context.prompt.clone()),
        expects_initial_agent: context.is_some(),
    };
    match worktrees.create(&input, &timestamp()) {
        Ok(created) => {
            if let Some(context) = context
                && let Some(source) = domain::workspace::naming::first_agent_source(
                    context.prompt.as_deref(),
                    &context.attachments,
                )
            {
                worktrees.name_workspace(created.workspace.workspace_id.clone(), source);
            }
            let descriptor = domain::workspace::protocol::projection::workspace_descriptor(
                &created.workspace,
                Some(&created.project),
            );
            let event = serde_json::json!({
                "kind": "upsert",
                "workspace": descriptor,
            });
            dispatched(
                WorktreeCreateResult {
                    workspace: Some(descriptor),
                    error: None,
                    error_code: None,
                    setup_terminal_id: None,
                    setup_skipped_reason: None,
                },
                Some(event),
                Some(created.workspace.workspace_id),
            )
        }
        Err(error) => dispatched(
            WorktreeCreateResult {
                workspace: None,
                error: Some(error.to_string()),
                error_code: Some(create_error_code(&error).to_owned()),
                setup_terminal_id: None,
                setup_skipped_reason: None,
            },
            None,
            None,
        ),
    }
}

fn archive(
    worktrees: &Worktrees,
    request: WorktreeArchiveRequest,
) -> Result<Dispatched, ErrorCode> {
    let input = archive_input(request);
    match worktrees.archive(&input, &timestamp()) {
        Ok(_) => value(WorktreeArchiveResult {
            success: true,
            removed_agents: Some(Vec::new()),
            error: None,
        }),
        Err(error) => value(WorktreeArchiveResult {
            success: false,
            removed_agents: Some(Vec::new()),
            error: Some(checkout_error(&error)),
        }),
    }
}

/// Translate a decoded archive request for the host's resource-close coordination.
#[must_use]
pub fn archive_input(request: WorktreeArchiveRequest) -> ArchiveWorktree {
    ArchiveWorktree {
        worktree_path: request.worktree_path,
        repo_root: request.repo_root,
        worktree_slug: None,
        branch_name: request.branch_name,
        workspace_id: request.workspace_id,
        scope: match request.scope {
            WorktreeArchiveScope::Workspace => ArchiveScope::Workspace,
            WorktreeArchiveScope::Worktree => ArchiveScope::Worktree,
        },
    }
}

/// Project a categorized worktree failure into its public, safe checkout error.
#[must_use]
pub fn checkout_error(error: &WorktreesError) -> CheckoutError {
    let code = checkout_error_code(error.kind());
    CheckoutError {
        code,
        message: error.to_string(),
    }
}

const fn checkout_error_code(kind: WorktreeFailureKind) -> CheckoutErrorCode {
    match kind {
        WorktreeFailureKind::NotGitRepository => CheckoutErrorCode::NotGitRepo,
        WorktreeFailureKind::NotAllowed => CheckoutErrorCode::NotAllowed,
        _ => CheckoutErrorCode::Unknown,
    }
}

fn create_error_code(error: &WorktreesError) -> &'static str {
    create_error_code_for_kind(error.kind())
}

const fn create_error_code_for_kind(kind: WorktreeFailureKind) -> &'static str {
    match kind {
        WorktreeFailureKind::BranchAlreadyCheckedOut => "branch_already_checked_out",
        WorktreeFailureKind::MissingCheckoutTarget => "missing_checkout_target",
        WorktreeFailureKind::UnknownBranch => "unknown_branch",
        _ => "unknown",
    }
}

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, ErrorCode> {
    serde_json::from_value(value).map_err(|_| ErrorCode::InvalidMessage)
}

fn value(value: impl Serialize) -> Result<Dispatched, ErrorCode> {
    dispatched(value, None, None)
}

fn dispatched(
    value: impl Serialize,
    event: Option<Value>,
    created_workspace_id: Option<String>,
) -> Result<Dispatched, ErrorCode> {
    Ok(Dispatched {
        value: serde_json::to_value(value).map_err(|_| ErrorCode::RegistryIo)?,
        event,
        created_workspace_id,
    })
}

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests;
