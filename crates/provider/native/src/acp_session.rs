//! Session lifecycle shared by stdio ACP adapters that drive one native session per process.

use std::collections::{BTreeMap, VecDeque};

use domain::agent_runtime::AgentPersistenceHandle;
use serde_json::{Value, json};

use super::acp_config::{mcp_servers, text};
use super::acp_permissions::{Pending, cancelled, resolve};
use super::acp_transport::Transport;
use provider_core::ports::agent_session::{AgentSessionError, AgentSessionSpec, AgentTurnEvent};

/// Longest native session identity a durable handle may carry.
const MAX_SESSION_ID: usize = 1024;

/// Native session established by the ACP handshake.
#[derive(Debug)]
pub struct Opened {
    /// Agent capabilities advertised by `initialize`.
    pub capabilities: Value,
    /// Native `session/new` or `session/resume` result, including its configuration options.
    pub result: Value,
    /// Native session identity that later requests address.
    pub id: String,
}

/// Check whether a durable handle can resume a native session in a working directory.
///
/// # Arguments
///
/// * `handle` - Durable handle recorded by [`persistence`].
/// * `provider` - Stable provider identity the handle must name.
/// * `cwd` - Working directory the resumed session will run in.
///
/// # Returns
/// `true` for a handle of `provider` whose session identity is bounded, non-empty and free of
/// control characters, and whose recorded directory equals `cwd`.
#[must_use]
pub fn resumable(handle: &AgentPersistenceHandle, provider: &str, cwd: &str) -> bool {
    handle.provider == provider
        && !handle.session_id.is_empty()
        && handle.session_id.len() <= MAX_SESSION_ID
        && !handle.session_id.chars().any(char::is_control)
        && handle
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("cwd"))
            .and_then(Value::as_str)
            == Some(cwd)
}

/// Build the durable handle that [`resumable`] accepts for the same directory.
///
/// # Arguments
///
/// * `provider` - Stable provider identity of the session.
/// * `session_id` - Native session identity.
/// * `cwd` - Working directory the session runs in.
///
/// # Returns
/// A handle without a native payload whose metadata records only `cwd`.
#[must_use]
pub fn persistence(provider: &str, session_id: &str, cwd: &str) -> AgentPersistenceHandle {
    AgentPersistenceHandle {
        provider: provider.to_owned(),
        session_id: session_id.to_owned(),
        native_handle: None,
        metadata: Some(BTreeMap::from([("cwd".to_owned(), json!(cwd))])),
    }
}

/// Initialize ACP v1 and create, or resume, one native session.
///
/// # Arguments
///
/// * `transport` - Fresh connection to the native agent process.
/// * `client_capabilities` - Capabilities Ait advertises in `initialize`.
/// * `spec` - Requested working directory and configuration, whose MCP servers attach here.
/// * `handle` - Durable handle to resume, already checked with [`resumable`]; none creates.
///
/// # Returns
/// The advertised agent capabilities, the native session result and its identity.
///
/// # Errors
///
/// Returns `AgentSessionError::Failed` for another protocol version or a result without a
/// session identity, `AgentSessionError::Rejected` when resuming an agent that does not
/// advertise resume or that reports a different session, and any transport error.
pub async fn open(
    transport: &mut Transport,
    client_capabilities: Value,
    spec: &AgentSessionSpec,
    handle: Option<&AgentPersistenceHandle>,
) -> Result<Opened, AgentSessionError> {
    let initialized = transport
        .request(
            "initialize",
            json!({"protocolVersion":1,"clientCapabilities":client_capabilities,
                "clientInfo":{"name":"ait","version":env!("CARGO_PKG_VERSION")}}),
        )
        .await?;
    if initialized["protocolVersion"] != 1 {
        return Err(AgentSessionError::Failed);
    }
    let capabilities = initialized["agentCapabilities"].clone();
    let mut params = json!({"cwd":spec.cwd,"mcpServers":mcp_servers(&spec.config)});
    let method = if let Some(handle) = handle {
        if !capabilities["sessionCapabilities"]["resume"].is_object() {
            return Err(AgentSessionError::Rejected);
        }
        params["sessionId"] = json!(handle.session_id);
        "session/resume"
    } else {
        "session/new"
    };
    let result = transport.request(method, params).await?;
    let id = match handle {
        Some(handle) => {
            // A resumed session must answer with the handle it was given, or the later
            // requests would address a session the agent does not have.
            if let Some(reported) = text(&result, "sessionId").ok()
                && reported != handle.session_id
            {
                return Err(AgentSessionError::Rejected);
            }
            handle.session_id.clone()
        }
        None => text(&result, "sessionId")?.to_owned(),
    };
    Ok(Opened {
        capabilities,
        result,
        id,
    })
}

/// Answer every unanswered callback without a choice so the agent can settle its prompt.
///
/// # Arguments
///
/// * `transport` - Connection to the native agent that sent the callbacks.
/// * `pending` - Callbacks awaiting a host answer; emptied by this call.
/// * `events` - Turn events that receive one `PermissionResolved` per withdrawn callback.
///
/// # Errors
///
/// Returns the transport error of a reply that could not be written; callbacks already
/// taken from `pending` are not restored.
pub async fn withdraw(
    transport: &mut Transport,
    pending: &mut BTreeMap<String, Pending>,
    events: &mut VecDeque<AgentTurnEvent>,
) -> Result<(), AgentSessionError> {
    for (id, retained) in std::mem::take(pending) {
        transport.send(&cancelled(&retained)).await?;
        events.push_back(AgentTurnEvent::PermissionResolved(id));
    }
    Ok(())
}

/// Send the validated native reply for one host answer and forget its callback.
///
/// # Arguments
///
/// * `transport` - Connection to the native agent that sent the callback.
/// * `pending` - Callbacks awaiting a host answer, keyed by host request identity.
/// * `events` - Turn events that receive the `PermissionResolved` event.
/// * `id` - Host request identity being answered.
/// * `response` - Host answer, validated by [`resolve`].
///
/// # Errors
///
/// Returns `AgentSessionError::Rejected` for an unknown request or an answer [`resolve`]
/// rejects, or the transport error of a reply that could not be written; the callback stays
/// pending in both cases.
pub async fn respond(
    transport: &mut Transport,
    pending: &mut BTreeMap<String, Pending>,
    events: &mut VecDeque<AgentTurnEvent>,
    id: &str,
    response: &Value,
) -> Result<(), AgentSessionError> {
    let retained = pending.get(id).ok_or(AgentSessionError::Rejected)?;
    transport.send(&resolve(retained, response)?).await?;
    pending.remove(id);
    events.push_back(AgentTurnEvent::PermissionResolved(id.to_owned()));
    Ok(())
}

/// Withdraw unanswered callbacks, then ask the agent to cancel the running prompt.
///
/// The correlated prompt response still settles the turn.
///
/// # Arguments
///
/// * `transport` - Connection to the native agent.
/// * `session` - Native session identity running the prompt.
/// * `pending` - Callbacks awaiting a host answer; emptied by this call.
/// * `events` - Turn events that receive one `PermissionResolved` per withdrawn callback.
///
/// # Errors
///
/// Returns the transport error of a reply or notification that could not be written.
pub async fn cancel(
    transport: &mut Transport,
    session: &str,
    pending: &mut BTreeMap<String, Pending>,
    events: &mut VecDeque<AgentTurnEvent>,
) -> Result<(), AgentSessionError> {
    withdraw(transport, pending, events).await?;
    transport
        .send(&json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":session}}))
        .await
}

/// Close the native session when the agent supports it, then stop its process.
///
/// # Arguments
///
/// * `transport` - Connection to the native agent; closed even when the request fails.
/// * `session` - Native session identity to close.
/// * `capabilities` - Agent capabilities advertised by `initialize`.
///
/// # Errors
///
/// Returns the `session/close` failure first, otherwise the process shutdown failure.
pub async fn close(
    transport: &mut Transport,
    session: &str,
    capabilities: &Value,
) -> Result<(), AgentSessionError> {
    let result = if capabilities["sessionCapabilities"]["close"].is_object() {
        transport
            .request("session/close", json!({"sessionId":session}))
            .await
            .map(|_| ())
    } else {
        Ok(())
    };
    let stopped = transport.close().await;
    result.and(stopped)
}

#[cfg(all(test, unix))]
mod tests;
