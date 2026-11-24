//! The calling side of calls between sibling Hosts of the account: `host_list`, and tool calls
//! a host-credential caller runs on another Host by naming it with `hostId`.
//!
//! Only host-credential callers are offered either: `host_list` is host-only, and only their
//! schemas of the tools marked for peers have `hostId`. A call naming this Host, or none, runs
//! here unchanged. A call naming another Host shares this API's call budget and time limit with
//! local calls, and every way it fails is a tool result naming the Host with a code. The rules of
//! the receiving side are in [`super::peer`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use model::peer::{Error, Host, HostState, Request};
use provider::service::agent_mcp::GUEST_SHARE_LABEL;
use serde_json::{Value, json};
use tokio::time::Instant;
use uuid::Uuid;

use crate::Shared;
use crate::connection::rpc;

use super::catalog::{HOST_ARGUMENT, Tool};
use super::scope::Scope;
use super::{CALL_TIMEOUT, admitted, caller_record, own_credential, peer, tool_result};

/// How long a call to another Host waits for its reply: less than `CALL_TIMEOUT`, so the Hub
/// link, which knows whether the request was sent, answers before this API gives up.
const PEER_CALL_TIMEOUT: Duration = Duration::from_secs(55);
const _: () = assert!(PEER_CALL_TIMEOUT.as_secs() < CALL_TIMEOUT.as_secs());
/// Least time worth sending a call with; with less left the call is refused before it is sent.
const MIN_PEER_CALL_TIME: Duration = Duration::from_secs(1);

/// Text of a call that was sent but whose reply never arrived.
const OUTCOME_UNKNOWN: &str = "The call may have run on the other host. Check with agent_list or \
    agent_get before retrying.";
/// Text of a call refused, unsent, while too many calls to other Hosts are open.
const CAPACITY: &str = "Too many calls to other hosts are open right now; this one was not \
    sent. Retry later.";
/// Text of a request too large to send; it never left this Host.
const REQUEST_TOO_LARGE: &str = "The request exceeds the 1 MiB message limit and was not sent. \
    Retrying unchanged fails the same way: send less.";
/// Text of a call that ran on the other Host but whose reply was too large to return.
const REPLY_TOO_LARGE: &str = "The call ran on the other host, but its reply exceeds the 1 MiB \
    message limit. Do not repeat a call that changes something; ask for less (for example a \
    lower `limit`).";
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

/// For a host caller's call of a tool marked for peers, take `hostId` out of `arguments` and
/// return the Host it names, unless that is this Host: a call naming this Host, or none, runs
/// here as if it never had one. Ids are compared as UUIDs, so any spelling of this Host's id
/// names it. Any other call is never routed and keeps its arguments, so a `hostId` its closed
/// schema does not offer is refused by validation, not stripped.
pub(super) fn destination(
    state: &Shared,
    scope: Option<&Scope>,
    tool: &Tool,
    arguments: &mut Value,
) -> Option<String> {
    if !tool.routable(scope) {
        return None;
    }
    let Value::String(host) = arguments.as_object_mut()?.remove(HOST_ARGUMENT)? else {
        return None;
    };
    let local = Uuid::parse_str(&state.info.server_id).ok();
    let here = local.is_some() && Uuid::parse_str(&host).ok() == local;
    (!here).then_some(host)
}

/// Run `tool` with `arguments` on Host `host` for this Host's `caller` Agent, under this API's
/// call budget and time limit, all within `PEER_CALL_TIMEOUT` from now.
///
/// # Returns
/// The other Host's result with `structuredContent.hostId` added, or a failed tool result
/// naming `host` with the code of the failure. A call this API gives up on once its request was
/// handed to the Hub link, because the server drains or the call limit passes, ends as
/// `outcome_unknown` like any other call whose reply is lost.
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
    let routed = Arc::new(Routed {
        host,
        deadline: Instant::now() + PEER_CALL_TIMEOUT,
        sent: AtomicBool::new(false),
    });
    let abandoned = routed.clone();
    let task_state = state.clone();
    let call =
        async move { forward(&task_state, tool, arguments, caller.as_deref(), &routed).await };
    admitted(state, None, call, move |code| abandoned.abandoned(code)).await
}

/// One call bound for another Host.
#[derive(Debug)]
struct Routed {
    /// The Host as the caller named it.
    host: String,
    /// When the reply must have arrived: `PEER_CALL_TIMEOUT` after the call was routed, which is
    /// before `CALL_TIMEOUT` runs out, since that starts only once the call is admitted.
    deadline: Instant,
    /// Whether the request was handed to the Hub link; from then on it may have run.
    sent: AtomicBool,
}

impl Routed {
    /// The result of this call when this API gives up on it with `code`: unknown once its
    /// request was handed over, since it may have run on the other Host, otherwise `code`.
    fn abandoned(&self, code: model::ErrorCode) -> Value {
        if self.sent.load(Ordering::Acquire) {
            refused(&self.host, &Error::OutcomeUnknown)
        } else {
            tool_result(Err(code.into()))
        }
    }
}

/// Check the caller and the target Host, then send the call once with what remains of its time;
/// a call that was sent is never sent again, whatever happens to its reply.
async fn forward(
    state: &Shared,
    tool: &Tool,
    arguments: Value,
    caller: Option<&str>,
    routed: &Routed,
) -> Result<Value, (i32, &'static str)> {
    let host = routed.host.as_str();
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
    // The checks above may have used most of the time, a slow uncached Host list above all.
    let remaining = routed.deadline.saturating_duration_since(Instant::now());
    if remaining < MIN_PEER_CALL_TIME {
        return Ok(refused(host, &Error::Unavailable));
    }
    let request = Request {
        tool: tool.name.clone(),
        arguments,
        caller: caller.map(str::to_owned),
    };
    // Marked sent before the Hub link has it: a drain while the session opens then reports
    // `outcome_unknown` for a call that never left. That is the safe direction, since the
    // opposite order could report "not sent" for a call that ran; keep it.
    routed.sent.store(true, Ordering::Release);
    Ok(
        match state.peers.call(target.node_id, request, remaining).await {
            Ok(result) => {
                tagged(result, host).unwrap_or_else(|| refused(host, &Error::OutcomeUnknown))
            }
            Err(error) => refused(host, &error),
        },
    )
}

/// Whether the caller Agent `agent` must stay on this Host. A sandboxed worktree confines what
/// its Agents write to this Host, which a call to another Host would escape, and a guest's
/// Agents never get this Host's MCP. Best effort: `X-Ait-Agent-Id` is not a permission boundary
/// (ADR-215). A Workspace `workspace.get` does not describe counts as sandboxed: one that cannot
/// be read, and one that is archived or missing, for which it answers without a `workspace`.
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

/// Add `hostId` to the `structuredContent` of the other Host's `result`, creating it when the
/// result has none, and leaving the rest as it was sent.
///
/// # Returns
/// `None` for a reply that is not a tool result (not an object, or with a `structuredContent`
/// that is not one). The Hub link already refuses such replies as malformed; should one still
/// arrive, the call ran but its answer cannot be read, so the caller learns its outcome is unknown.
fn tagged(mut result: Value, host: &str) -> Option<Value> {
    result
        .as_object_mut()?
        .entry("structuredContent")
        .or_insert_with(|| json!({}))
        .as_object_mut()?
        .insert(HOST_ARGUMENT.to_owned(), json!(host));
    Some(result)
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
        Error::Capacity => failed(host, "capacity", CAPACITY),
        Error::RequestTooLarge => failed(host, "capacity", REQUEST_TOO_LARGE),
        Error::ReplyTooLarge => failed(host, "capacity", REPLY_TOO_LARGE),
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

#[cfg(test)]
mod tests;
