//! Transport-independent workspace state request handling.

/// Client methods implemented by this component; consumed by capability discovery.
pub const METHODS: &[MethodSpec] = &[
    MethodSpec::request("workspace.clear_attention.request"),
    MethodSpec::request("workspace.mark_unread.request"),
];

use model::methods::MethodSpec;
use serde::Serialize;
use serde_json::Value;

use crate::directory::protocol::workspace_state::{
    WorkspaceClearAttentionItem, WorkspaceClearAttentionRequest, WorkspaceClearAttentionResult,
    WorkspaceIdSelection, WorkspaceMarkUnreadRequest, WorkspaceMarkUnreadResult,
};
use crate::directory::service::workspace_state::{WorkspaceAttentionBatch, WorkspaceState};
use crate::support::error::ErrorCode;
use crate::support::rpc::{decode, encode};
use crate::support::time::now_timestamp;

/// Completed Workspace state request.
#[derive(Debug)]
pub(crate) struct Dispatched {
    /// Response payload.
    pub(crate) value: Value,
    /// Optional Workspace event sent after the response.
    pub(crate) event: Option<Value>,
}

/// Decode and execute a business request.
///
/// # Errors
/// Returns stable business failures for invalid or unsuccessful requests.
pub(crate) fn execute(
    workspace_state: &mut WorkspaceState,
    method: &str,
    params: Value,
) -> Result<Dispatched, ErrorCode> {
    match method {
        "workspace.clear_attention.request" => clear_attention(workspace_state, decode(params)?),
        "workspace.mark_unread.request" => mark_unread(workspace_state, decode(params)?),
        _ => Err(ErrorCode::MethodNotFound),
    }
}

fn clear_attention(
    workspace_state: &WorkspaceState,
    request: WorkspaceClearAttentionRequest,
) -> Result<Dispatched, ErrorCode> {
    let ids = match &request.workspace_id {
        WorkspaceIdSelection::One(workspace_id) => vec![workspace_id.clone()],
        WorkspaceIdSelection::Many(workspace_ids) => workspace_ids.clone(),
    };
    let result = workspace_state.clear_attention(&ids, &now_timestamp());
    value(clear_attention_result(request.workspace_id, result))
}

fn mark_unread(
    workspace_state: &WorkspaceState,
    request: WorkspaceMarkUnreadRequest,
) -> Result<Dispatched, ErrorCode> {
    let result = match workspace_state.mark_unread(&request.workspace_id, &now_timestamp()) {
        Ok(agent_id) => WorkspaceMarkUnreadResult {
            workspace_id: request.workspace_id,
            marked_agent_id: Some(agent_id),
            success: true,
            error: None,
        },
        Err(error) => WorkspaceMarkUnreadResult {
            workspace_id: request.workspace_id,
            marked_agent_id: None,
            success: false,
            error: Some(error.to_string()),
        },
    };
    value(result)
}

fn clear_attention_result(
    workspace_id: WorkspaceIdSelection,
    result: WorkspaceAttentionBatch,
) -> WorkspaceClearAttentionResult {
    WorkspaceClearAttentionResult {
        workspace_id,
        cleared_agent_ids: result.cleared_agent_ids,
        results: result
            .results
            .into_iter()
            .map(|item| WorkspaceClearAttentionItem {
                workspace_id: item.workspace_id,
                cleared_agent_ids: item.cleared_agent_ids,
                success: item.success,
                error: item.error,
            })
            .collect(),
        success: result.success,
        error: result.error,
    }
}

fn value(value: impl Serialize) -> Result<Dispatched, ErrorCode> {
    dispatched(value, None)
}

fn dispatched(value: impl Serialize, event: Option<Value>) -> Result<Dispatched, ErrorCode> {
    Ok(Dispatched {
        value: encode(value, ErrorCode::RegistryIo)?,
        event,
    })
}

#[cfg(test)]
mod tests;
