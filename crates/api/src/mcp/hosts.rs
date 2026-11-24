//! The calling side of calls between sibling Hosts of the account: `host_list`, and tool calls
//! a host-credential caller runs on another Host by naming it with `hostId`.
//!
//! Only host-credential callers are offered either: `host_list` is host-only, and only their
//! schemas of the tools marked for peers have `hostId`. A call naming this Host, or none, runs
//! here unchanged. A call naming another Host shares this API's call budget and time limit with
//! local calls, and every way it fails is a tool result naming the Host with a code. The rules of
//! the receiving side are in [`super::peer`].

use std::sync::Arc;
use std::time::Duration;

use model::peer::{Error, Host, HostState, Request};
use provider::service::agent_mcp::GUEST_SHARE_LABEL;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::Shared;
use crate::connection::rpc;

use super::catalog::{HOST_ARGUMENT, Tool};
use super::{CALL_TIMEOUT, admitted, caller_record, own_credential, peer, tool_result};

/// How long a call to another Host waits for its reply: less than `CALL_TIMEOUT`, so the Hub
/// link, which knows whether the request was sent, answers before this API gives up.
const PEER_CALL_TIMEOUT: Duration = Duration::from_secs(55);
const _: () = assert!(PEER_CALL_TIMEOUT.as_secs() < CALL_TIMEOUT.as_secs());

/// Text of a call that was sent but whose reply never arrived.
const OUTCOME_UNKNOWN: &str = "The call may have run on the other host. Check with agent_list or \
    agent_get before retrying.";
/// Text of a refused caller Agent.
const CALLER_NOT_ALLOWED: &str = "Agents in a sandboxed worktree and Agents a guest created \
    cannot call other hosts.";
/// Note of a `host_list` without the account's other Hosts.
const UNLISTED: &str = "Only this host is listed: the account hub is not reachable or cannot \
    list the other hosts of this account.";

/// Why a listed Host cannot take a call now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Blocked {
    /// It is not connected to the account center.
    Offline,
    /// Its version cannot receive calls from other Hosts.
    Unsupported,
    /// It does not accept calls from other Hosts.
    NotAccepting,
}

impl Blocked {
    /// Why `host` cannot take a call now, checked in this order; `None` when it can.
    fn of(host: &Host) -> Option<Self> {
        if host.state != HostState::Online {
            Some(Self::Offline)
        } else if !host.supported {
            Some(Self::Unsupported)
        } else if !host.enabled {
            Some(Self::NotAccepting)
        } else {
            None
        }
    }

    /// The `reason` `host_list` gives.
    const fn reason(self) -> &'static str {
        match self {
            Self::Offline => "offline",
            Self::Unsupported => "unsupported",
            Self::NotAccepting => "not_accepting",
        }
    }

    /// The failure a call to such a Host ends with, without being sent.
    const fn error(self) -> Error {
        match self {
            Self::Offline => Error::Offline,
            Self::Unsupported => Error::Unsupported,
            Self::NotAccepting => Error::NotAccepting,
        }
    }
}

/// The `host_list` result.
///
/// # Returns
/// `accepting`, whether this Host accepts calls from sibling Hosts now, and `hosts`: this Host
/// first, always `available`, then the account's other Hosts as the center lists them, each
/// with `available` and, when false, `reason`. When the center cannot list them, only this Host
/// is listed, with what it knows of itself, and `note` says why.
pub(super) async fn list(state: &Shared) -> Value {
    let accepting = peer::accepts(state).await;
    let local = state.info.server_id.as_str();
    match state.peers.hosts().await {
        Ok(hosts) => json!({"accepting":accepting,"hosts":listing(local, &hosts)}),
        Err(error) => {
            tracing::debug!(%error, "Other Hosts of the account cannot be listed");
            json!({"accepting":accepting,"hosts":[unlisted(local)],"note":UNLISTED})
        }
    }
}

/// List `hosts` with the one whose `serverId` is `local` first.
fn listing(local: &str, hosts: &[Host]) -> Vec<Value> {
    let id = Uuid::parse_str(local).ok();
    let mut listed = Vec::with_capacity(hosts.len() + 1);
    let this = hosts.iter().find(|host| Some(host.server_id) == id);
    listed.push(this.map_or_else(|| unlisted(local), |host| entry(host, true)));
    listed.extend(
        hosts
            .iter()
            .filter(|host| Some(host.server_id) != id)
            .map(|host| entry(host, false)),
    );
    listed
}

/// The `host_list` entry of `host`; `this` marks this Host, which is always available.
fn entry(host: &Host, this: bool) -> Value {
    let mut entry = json!({
        "hostId":host.server_id.to_string(),"name":host.name,"hostname":host.hostname,
        "platform":host.platform,"appVersion":host.app_version,"state":state_name(host.state),
        "self":this,"available":true,
    });
    if let Some(blocked) = Blocked::of(host).filter(|_| !this) {
        entry["available"] = json!(false);
        entry["reason"] = json!(blocked.reason());
    }
    entry
}

/// The entry of this Host when the center does not list it: only its id and version are known.
fn unlisted(local: &str) -> Value {
    json!({
        "hostId":local,"name":null,"hostname":null,"platform":null,
        "appVersion":model::BUILD_VERSION,"state":null,"self":true,"available":true,
    })
}

const fn state_name(state: HostState) -> &'static str {
    match state {
        HostState::Online => "online",
        HostState::Reconnecting => "reconnecting",
        HostState::Offline => "offline",
    }
}

/// Take `hostId` out of `arguments` and return the Host it names, unless that is this Host: a
/// call naming this Host, or none, runs here as if it never had one. Ids are compared as UUIDs,
/// so any spelling of this Host's id names it.
pub(super) fn destination(state: &Shared, arguments: &mut Value) -> Option<String> {
    let Value::String(host) = arguments.as_object_mut()?.remove(HOST_ARGUMENT)? else {
        return None;
    };
    let local = Uuid::parse_str(&state.info.server_id).ok();
    let here = local.is_some() && Uuid::parse_str(&host).ok() == local;
    (!here).then_some(host)
}

/// Run `tool` with `arguments` on Host `host` for this Host's `caller` Agent, under this API's
/// call budget and time limit.
///
/// # Returns
/// The other Host's result with `structuredContent.hostId` added, or a failed tool result
/// naming `host` with the code of the failure.
///
/// # Errors
/// Returns the refusals of a local call this API cannot run now, an unknown caller Agent as a
/// failed tool result, and `-32602` for host credentials acting for a Bot-scoped Agent.
pub(super) async fn route(
    state: Arc<Shared>,
    tool: &'static Tool,
    arguments: Value,
    caller: Option<String>,
    host: String,
) -> Result<Value, (i32, &'static str)> {
    let task_state = state.clone();
    let call = async move { forward(&task_state, tool, arguments, caller.as_deref(), &host).await };
    admitted(state, None, call).await
}

/// Check the caller and the target Host, then send the call once; a call that was sent is never
/// sent again, whatever happens to its reply.
async fn forward(
    state: &Shared,
    tool: &Tool,
    arguments: Value,
    caller: Option<&str>,
    host: &str,
) -> Result<Value, (i32, &'static str)> {
    let context = match caller_record(state, caller).await {
        Ok(context) => context,
        Err(error) => return Ok(tool_result(Err(error.into()))),
    };
    own_credential(None, context.as_ref())?;
    if let Some(agent) = &context
        && confined(state, agent).await
    {
        return Ok(failed(host, "caller_not_allowed", CALLER_NOT_ALLOWED));
    }
    let target = match resolve(state, host).await {
        Ok(target) => target,
        Err(error) => return Ok(refused(host, &error)),
    };
    if let Some(blocked) = Blocked::of(&target) {
        return Ok(refused(host, &blocked.error()));
    }
    let request = Request {
        tool: tool.name.clone(),
        arguments,
        caller: caller.map(str::to_owned),
    };
    Ok(
        match state
            .peers
            .call(target.node_id, request, PEER_CALL_TIMEOUT)
            .await
        {
            Ok(result) => tagged(result, host),
            Err(error) => refused(host, &error),
        },
    )
}

/// Whether the caller Agent `agent` must stay on this Host. A sandboxed worktree confines what
/// its Agents write to this Host, which a call to another Host would escape, and a guest's
/// Agents never get this Host's MCP. Best effort: `X-Ait-Agent-Id` is not a permission boundary
/// (ADR-215). A Workspace that cannot be read, or is archived, counts as sandboxed.
async fn confined(state: &Shared, agent: &Value) -> bool {
    if agent["labels"].get(GUEST_SHARE_LABEL).is_some() {
        return true;
    }
    let Some(workspace) = agent.get("workspaceId").filter(|id| !id.is_null()) else {
        return false;
    };
    match rpc::execute(
        state,
        "workspace.get.request",
        json!({"workspaceId":workspace}),
    )
    .await
    {
        Ok(found) => found["workspace"].as_object().is_none_or(|workspace| {
            workspace
                .get("sandbox")
                .is_some_and(|level| !level.is_null())
        }),
        Err(error) => {
            tracing::warn!(
                ?error,
                "Caller Workspace unreadable; refusing a call to another host"
            );
            true
        }
    }
}

/// The account's Host whose `serverId` is `host`, from the center's list.
/// # Errors
/// Returns `UnknownHost` when no Host has that id, and the list's own failure.
async fn resolve(state: &Shared, host: &str) -> Result<Host, Error> {
    let id = Uuid::parse_str(host).map_err(|_| Error::UnknownHost)?;
    state
        .peers
        .hosts()
        .await?
        .into_iter()
        .find(|listed| listed.server_id == id)
        .ok_or(Error::UnknownHost)
}

/// Add `hostId` to the `structuredContent` of the other Host's `result`, leaving the rest as it
/// was sent.
fn tagged(mut result: Value, host: &str) -> Value {
    if let Some(result) = result.as_object_mut()
        && let Some(structured) = result
            .entry("structuredContent")
            .or_insert_with(|| json!({}))
            .as_object_mut()
    {
        structured.insert(HOST_ARGUMENT.to_owned(), json!(host));
    }
    result
}

/// The failed tool result of a call to `host` that ended with `error`.
fn refused(host: &str, error: &Error) -> Value {
    match error {
        Error::Rejected { code, message } => {
            let mut result = failed(host, "host_rejected", message);
            result["structuredContent"]["remoteCode"] = json!(code);
            result
        }
        Error::OutcomeUnknown => failed(host, "outcome_unknown", OUTCOME_UNKNOWN),
        Error::Unavailable => failed(host, "hub_unavailable", &error.to_string()),
        Error::UnknownHost => failed(host, "host_unknown", &error.to_string()),
        Error::Offline => failed(host, "host_offline", &error.to_string()),
        Error::NotAccepting => failed(host, "host_not_accepting", &error.to_string()),
        Error::Unsupported => failed(host, "host_unsupported", &error.to_string()),
        Error::Capacity | Error::TooLarge => failed(host, "capacity", &error.to_string()),
    }
}

/// A failed tool result naming `host`, with `code` and the text `error`.
fn failed(host: &str, code: &str, error: &str) -> Value {
    json!({
        "content":[{"type":"text","text":error}],
        "structuredContent":{"hostId":host,"code":code,"error":error},
        "isError":true,
    })
}
