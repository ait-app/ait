//! Tool calls from sibling Hosts of the account: a third kind of MCP caller.
//!
//! A sibling Host is authenticated by the account center, not by anything in its request. It may
//! call only the tools marked for peers, with the arguments a host-credential caller may pass
//! (never `hostId`: there is no second hop), and has no caller Agent on this Host to inherit
//! placement from. Within those rules it runs as an unscoped caller without caller context,
//! sharing the API's call budget and time limit with local MCP calls.

use std::sync::Arc;

use model::peer::{Identity, Request};
use relay::hub::bridge::PeerRefusal;
use serde_json::Value;

use crate::Shared;
use crate::peer_acceptance::accepting;

use super::scope::{self, Failure};
use super::{DRAINING, UNAVAILABLE, catalog, daemon_config, run, tool_result, validation};

/// Peer-wire error code for a call this Host cannot run now, because its call budget is full or
/// it is draining: the call did not run, unlike a `-32000` reply (the call ran but its reply
/// exceeds the message size limit). The calling Host receives it as `Rejected` with the text.
pub(crate) const BUSY: i32 = -32_002;

/// Label naming the `serverId` of the Host whose Agent created this one.
pub(crate) const PARENT_SERVER_LABEL: &str = "ait.parent.server-id";
/// Label naming the Agent on that Host which created this one.
pub(crate) const PARENT_AGENT_LABEL: &str = "ait.parent.agent-id";
/// Prefix of the remote parent labels, which only this Host writes.
pub(crate) const PARENT_LABEL_PREFIX: &str = "ait.parent.";
/// Label namespaces a sibling Host may not write.
const RESERVED_LABEL_PREFIXES: [&str; 2] = ["ait.", "paseo."];

/// Refusal for an `agent_create` without a Workspace.
const UNPLACED_AGENT: &str = "agent_create from another host requires workspaceId, a Workspace \
    on this host: there is no caller Agent here to inherit one from";
/// Refusal for an `agent_create` naming a directory.
const AGENT_DIRECTORY: &str = "agent_create from another host does not accept cwd; pass the \
    workspaceId of a Workspace on this host instead";
/// Refusal for a `workspace_create` without a source.
const UNSOURCED_WORKSPACE: &str = "workspace_create from another host requires path or \
    projectId on this host: there is no caller Agent here to inherit a directory from";

/// Run one tool call that sibling Host `from` sent, under the rules for peers.
///
/// # Arguments
/// * `state` - This API.
/// * `from` - The calling Host, as the account center authenticated it.
/// * `request` - The tool call, with the id of the calling Agent on `from` when known; an empty
///   id counts as unknown.
///
/// # Returns
/// The same result object a local `tools/call` returns; rule violations of the call itself are
/// results with `isError: true`. An `agent_create` with a calling Agent records it as the remote
/// parent of the new Agent.
///
/// # Errors
/// Returns `PeerRefusal::Disabled` unless `mcp.enabled` and `mcp.acceptPeers` are both on now;
/// `Failed { code: -32602 }` for a tool not marked for peers or not installed, and for arguments
/// outside the host-credential schema or naming a Host; `Failed { code: BUSY }` when the call
/// budget is exhausted or the server drains.
pub(crate) async fn serve(
    state: &Arc<Shared>,
    from: &Identity,
    request: Request,
) -> Result<Value, PeerRefusal> {
    // A draining server can no longer read its configuration; the call did not run.
    if state.cancellation.is_cancelled() {
        return Err(failed(BUSY, DRAINING));
    }
    if !accepts(state).await {
        return Err(PeerRefusal::Disabled);
    }
    let Request {
        tool,
        mut arguments,
        caller,
    } = request;
    let tool = catalog::for_peers(state, &tool)
        .ok_or_else(|| failed(-32602, "Unknown or unavailable tool"))?;
    if !permitted(&tool.schema(None), &arguments) {
        return Err(failed(
            -32602,
            "Arguments do not match the tool input schema",
        ));
    }
    if let Err(refusal) = admit(&tool.name, &arguments) {
        return Ok(tool_result(Err(refusal)));
    }
    if tool.name == "agent_create"
        && let Some(caller) = caller.filter(|caller| !caller.is_empty())
        && let Err(error) = record_parent(from, caller, &mut arguments)
    {
        return Ok(tool_result(Err(error.into())));
    }
    run(state.clone(), tool, arguments, None, None)
        .await
        .map_err(|(code, message)| match code {
            UNAVAILABLE => failed(BUSY, message),
            code => failed(code, message),
        })
}

/// Whether a peer may pass `arguments` to a tool whose host-credential `schema` is given: never
/// with `hostId`, even once that schema offers it, since a call is not forwarded again.
fn permitted(schema: &Value, arguments: &Value) -> bool {
    arguments.get(catalog::HOST_ARGUMENT).is_none() && validation::matches(schema, arguments)
}

/// Whether this Host accepts calls from sibling Hosts now, read from the current configuration
/// rather than what was last published: the center may still deliver calls queued before this
/// Host stopped accepting. An unreadable configuration does not accept. `host_list` reports it.
pub(super) async fn accepts(state: &Shared) -> bool {
    match daemon_config(state).await {
        Ok(config) => config.is_some_and(|config| accepting(&config)),
        Err(error) => {
            tracing::warn!(
                ?error,
                "Daemon configuration unreadable; refusing a peer call"
            );
            false
        }
    }
}

/// Refuse what a peer cannot have: placement it would inherit from a caller Agent here, and
/// labels in namespaces this Host owns.
/// # Errors
/// Returns a guided refusal for an `agent_create` without `workspaceId` or with `cwd` and for a
/// `workspace_create` without `path` or `projectId`, and `Forbidden` for reserved labels.
fn admit(tool: &str, arguments: &Value) -> Result<(), Failure> {
    match tool {
        "agent_create" if arguments.get("workspaceId").is_none() => {
            return Err(Failure::Refused(UNPLACED_AGENT));
        }
        "agent_create" if arguments.get("cwd").is_some() => {
            return Err(Failure::Refused(AGENT_DIRECTORY));
        }
        "workspace_create"
            if arguments.get("path").is_none() && arguments.get("projectId").is_none() =>
        {
            return Err(Failure::Refused(UNSOURCED_WORKSPACE));
        }
        _ => {}
    }
    for prefix in RESERVED_LABEL_PREFIXES {
        scope::reserved_labels(arguments, prefix)?;
    }
    Ok(())
}

/// Label the Agent an `agent_create` makes with its remote parent: Agent `caller` on Host `from`.
/// # Errors
/// Returns `InvalidMessage` when `arguments` or its labels are not objects.
fn record_parent(
    from: &Identity,
    caller: String,
    arguments: &mut Value,
) -> Result<(), model::ErrorCode> {
    let labels = scope::object(arguments, "labels")?;
    labels.insert(
        PARENT_SERVER_LABEL.to_owned(),
        Value::String(from.server_id.to_string()),
    );
    labels.insert(PARENT_AGENT_LABEL.to_owned(), Value::String(caller));
    Ok(())
}

fn failed(code: i32, message: &str) -> PeerRefusal {
    PeerRefusal::Failed {
        code,
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests;
