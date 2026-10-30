//! Guest requests executed once without subscriptions so their replies can be confined.

use model::ErrorCode;
use serde_json::{Value, json};

use super::{Session, object, present};
use crate::Shared;

/// Reply post-processing for a privately executed guest request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Private {
    /// Keep only the guest's workspace.
    WorkspaceList,
    /// Remember the created terminal so revocation can end it.
    TerminalCreate,
}

/// Prepare a workspace listing: no live subscription, paging or synchronization cursor,
/// because those would describe the whole directory.
pub(super) fn workspace_list(mut params: Value) -> Result<Value, ErrorCode> {
    let request = object(&mut params)?;
    if present(request.get("subscribe")) {
        return Err(ErrorCode::Forbidden);
    }
    for key in ["subscribe", "page", "sync"] {
        request.remove(key);
    }
    Ok(params)
}

/// Keep only the guest's workspace in a listing, discarding everything else.
pub(super) fn filter_workspaces(listing: &Value, workspace_id: &str) -> Value {
    let entries: Vec<&Value> = listing["entries"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter(|entry| entry["id"] == workspace_id)
                .collect()
        })
        .unwrap_or_default();
    json!({
        "entries": entries,
        "emptyProjects": [],
        "pageInfo": {"nextCursor": null, "prevCursor": null, "hasMore": false},
    })
}

/// Execute an admitted private request and confine its reply.
///
/// # Errors
/// Returns the dispatch error of the underlying request.
pub(crate) async fn execute(
    session: &Session,
    state: &Shared,
    method: &str,
    params: Value,
    private: Private,
) -> Result<Value, ErrorCode> {
    let reply = crate::connection::rpc::execute(state, method, params).await?;
    match private {
        Private::WorkspaceList => Ok(filter_workspaces(&reply, session.workspace_id())),
        Private::TerminalCreate => {
            let Some(id) = reply["terminal"]["id"].as_str() else {
                return Ok(reply);
            };
            if !session.registration.record_terminal(id.to_owned()) {
                // The share was revoked while the terminal started.
                terminal::dispatch::revoke(&state.terminal, vec![id.to_owned()]).await?;
                return Err(ErrorCode::Forbidden);
            }
            session.remember_terminal(id);
            Ok(reply)
        }
    }
}
