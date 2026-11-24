//! Messages of `k:"peer"` sessions between sibling Hosts of one account
//! (`ait-hub-v1` feature `peer`), in both directions.
//!
//! The `open` and `accept` payloads declare the version (`{"v":1}`). Every later
//! message is one UTF-8 JSON text: a request
//! `{"id","tool","arguments","caller"?:{"agentId"}}`, or its reply
//! `{"id","result"}` / `{"id","error":{"code","message"}}`. The opener's side
//! (sessions this Host opens to call sibling Hosts) is in [`opener`].

use std::time::Duration;

use model::peer::{Error, Host, HostState, Peers, Request};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Bytes;
use uuid::Uuid;

use super::Connector;
use super::bridge::PeerRefusal;
use super::wire;

pub(super) mod opener;

/// Message version of peer sessions.
pub(super) const VERSION: u64 = 1;
/// Payload of a peer `open` and `accept`.
pub(super) const HELLO: &[u8] = br#"{"v":1}"#;
/// Error code of the reply to a call this Host does not accept; the session
/// then closes as `peer_disabled`.
pub(super) const NOT_ACCEPTING: i32 = -32_001;
/// Error code of the reply that replaces one over the message size limit.
pub(super) const TOO_LARGE: i32 = -32_000;
/// Message of the error that replaces a reply over the message size limit.
const TOO_LARGE_MESSAGE: &str = "reply exceeds the message size limit";
/// Center method listing the account's Hosts.
const LIST: &str = "hub.peers.list";
/// How long a Host list is reused on the same logical link.
const HOSTS_TTL: Duration = Duration::from_secs(5);

/// The latest Host list, for the logical link it was read on.
#[derive(Debug)]
pub(super) struct Listed {
    link: Option<Uuid>,
    at: Instant,
    hosts: Vec<Host>,
}

/// A peer message that is not what the protocol defines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("malformed peer message")]
pub(super) struct Malformed;

/// The version declaration of an `open` or `accept` payload.
#[derive(Deserialize)]
struct Hello {
    v: u64,
}

/// A request; its `arguments` must be an object.
#[derive(Serialize, Deserialize)]
struct WireRequest {
    id: u64,
    tool: String,
    arguments: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    caller: Option<Caller>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Caller {
    agent_id: String,
}

#[derive(Serialize, Deserialize)]
struct Reply {
    id: u64,
    #[serde(flatten)]
    outcome: Outcome,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Outcome {
    Result(Value),
    Error { code: i32, message: String },
}

/// The result of `hub.peers.list`.
#[derive(Deserialize)]
struct HostList {
    hosts: Vec<ListedHost>,
}

/// One Host as `hub.peers.list` describes it. A Host that never connected may
/// lack a node or `server_id`; its other descriptive fields may be null.
#[derive(Deserialize)]
struct ListedHost {
    host_id: Uuid,
    node_id: Option<Uuid>,
    server_id: Option<Uuid>,
    name: String,
    hostname: Option<String>,
    platform: Option<String>,
    app_version: Option<String>,
    state: ListedState,
    peer: ListedPeer,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum ListedState {
    Online,
    Reconnecting,
    /// Also any state a later center adds: such a Host is not known to be reachable.
    #[serde(other)]
    Offline,
}

#[derive(Deserialize)]
struct ListedPeer {
    supported: bool,
    enabled: bool,
}

/// Check that an `open` or `accept` payload declares this Host's version.
///
/// # Errors
/// Returns [`Malformed`] for any other payload.
pub(super) fn decode_hello(payload: &[u8]) -> Result<(), Malformed> {
    match serde_json::from_slice::<Hello>(payload) {
        Ok(Hello { v: VERSION }) => Ok(()),
        Ok(_) | Err(_) => Err(Malformed),
    }
}

/// Decode one request message.
///
/// # Returns
/// The request id, unique among the session's requests, and the call.
///
/// # Errors
/// Returns [`Malformed`] for text that is not a request.
pub(super) fn decode_request(text: &[u8]) -> Result<(u64, Request), Malformed> {
    let request: WireRequest = serde_json::from_slice(text).map_err(|_| Malformed)?;
    if !request.arguments.is_object() {
        return Err(Malformed);
    }
    Ok((
        request.id,
        Request {
            tool: request.tool,
            arguments: request.arguments,
            caller: request.caller.map(|caller| caller.agent_id),
        },
    ))
}

/// Encode the reply to one request.
///
/// A reply over the payload limit is replaced by a [`TOO_LARGE`] error. The
/// opener must open the session with a window of at least the payload limit
/// (`credit::WINDOW`): a reply larger than its window would end the session as
/// `overflow` instead.
///
/// # Arguments
/// * `id` - The request's id.
/// * `outcome` - The call's result object, or an error code and message.
///
/// # Returns
/// The reply message's UTF-8 JSON text.
pub(super) fn encode_reply(id: u64, outcome: Result<Value, (i32, String)>) -> Bytes {
    let outcome = match outcome {
        Ok(result) => Outcome::Result(result),
        Err((code, message)) => Outcome::Error { code, message },
    };
    let encoded = serialize(&Reply { id, outcome });
    if encoded.len() <= wire::MAX_PAYLOAD {
        return encoded.into();
    }
    let outcome = Outcome::Error {
        code: TOO_LARGE,
        message: TOO_LARGE_MESSAGE.to_owned(),
    };
    serialize(&Reply { id, outcome }).into()
}

/// The error a refused call is answered with.
///
/// # Arguments
/// * `refusal` - Why the local API did not run the call.
///
/// # Returns
/// [`NOT_ACCEPTING`] when this Host does not accept calls from other Hosts,
/// otherwise the code and message the API gave.
pub(super) fn refusal(refusal: PeerRefusal) -> (i32, String) {
    match refusal {
        PeerRefusal::Disabled => (NOT_ACCEPTING, refusal.to_string()),
        PeerRefusal::Failed { code, message } => (code, message),
    }
}

/// Encode one request.
///
/// # Arguments
/// * `id` - The request's id, unique among the session's requests.
/// * `request` - The call; its `arguments` must be an object, which the
///   receiver checks.
///
/// # Returns
/// The request message's UTF-8 JSON text.
pub(super) fn encode_request(id: u64, request: Request) -> Bytes {
    let request = WireRequest {
        id,
        tool: request.tool,
        arguments: request.arguments,
        caller: request.caller.map(|agent_id| Caller { agent_id }),
    };
    serde_json::to_vec(&request)
        .expect("a request of JSON values always serializes")
        .into()
}

/// Decode one reply.
///
/// # Returns
/// The id of the request it answers, and the `result` object or the error the
/// reply carries: [`NOT_ACCEPTING`] is `Error::NotAccepting`, [`TOO_LARGE`] is
/// `Error::TooLarge`, and any other code is the call's own `Error::Rejected`.
///
/// # Errors
/// Returns [`Malformed`] for text that is not a reply.
pub(super) fn decode_reply(text: &[u8]) -> Result<(u64, Result<Value, Error>), Malformed> {
    let reply: Reply = serde_json::from_slice(text).map_err(|_| Malformed)?;
    let outcome = match reply.outcome {
        Outcome::Result(result) => Ok(result),
        Outcome::Error {
            code: NOT_ACCEPTING,
            ..
        } => Err(Error::NotAccepting),
        Outcome::Error {
            code: TOO_LARGE, ..
        } => Err(Error::TooLarge),
        Outcome::Error { code, message } => Err(Error::Rejected { code, message }),
    };
    Ok((reply.id, outcome))
}

/// Decode the result of `hub.peers.list`.
///
/// A Host without a node or `server_id` has never connected and cannot be
/// addressed, so it is left out; missing descriptive fields read as empty.
///
/// # Errors
/// Returns [`Malformed`] for a result that is not a Host list.
pub(super) fn decode_hosts(result: Value) -> Result<Vec<Host>, Malformed> {
    let list: HostList = serde_json::from_value(result).map_err(|_| Malformed)?;
    Ok(list
        .hosts
        .into_iter()
        .filter_map(|host| {
            Some(Host {
                server_id: host.server_id?,
                host_id: host.host_id,
                node_id: host.node_id?,
                name: host.name,
                hostname: host.hostname.unwrap_or_default(),
                platform: host.platform.unwrap_or_default(),
                app_version: host.app_version.unwrap_or_default(),
                state: match host.state {
                    ListedState::Online => HostState::Online,
                    ListedState::Reconnecting => HostState::Reconnecting,
                    ListedState::Offline => HostState::Offline,
                },
                supported: host.peer.supported,
                enabled: host.peer.enabled,
            })
        })
        .collect())
}

/// Sibling Hosts as the serve link reaches them: the Host list from the
/// center, and calls over the peer sessions the link opens.
#[async_trait::async_trait]
impl Peers for Connector {
    /// The account's Hosts from `hub.peers.list`, reused for [`HOSTS_TTL`] on
    /// the same logical link.
    ///
    /// # Errors
    /// Returns `Error::Unavailable` when the center does not offer the method
    /// on this link, or the call or its result fails.
    async fn hosts(&self) -> Result<Vec<Host>, Error> {
        let (scope, link) = {
            let publication = self.publication.borrow();
            if !publication.supports(LIST) {
                return Err(Error::Unavailable);
            }
            let scope = publication.scope.clone().ok_or(Error::Unavailable)?;
            (scope, publication.link)
        };
        if let Some(listed) = &self.state.lock().await.hosts
            && listed.link == link
            && listed.at.elapsed() < HOSTS_TTL
        {
            return Ok(listed.hosts.clone());
        }
        let at = Instant::now();
        let result = self
            .call_center(scope, LIST, json!({}))
            .await
            .map_err(|_| Error::Unavailable)?;
        let hosts = decode_hosts(result).map_err(|_| Error::Unavailable)?;
        self.state.lock().await.hosts = Some(Listed {
            link,
            at,
            hosts: hosts.clone(),
        });
        Ok(hosts)
    }

    /// Send one call over this link's peer session to `node`, giving up after `timeout`.
    ///
    /// # Errors
    /// Returns `Error::Unavailable` without a link that enabled `peer`, and
    /// otherwise the outcome described on [`opener`].
    async fn call(&self, node: Uuid, request: Request, timeout: Duration) -> Result<Value, Error> {
        let caller = self.state.lock().await.peers.clone();
        caller
            .ok_or(Error::Unavailable)?
            .call(node, request, timeout)
            .await
    }
}

fn serialize(reply: &Reply) -> Vec<u8> {
    serde_json::to_vec(reply).expect("a reply of JSON values always serializes")
}

#[cfg(test)]
mod tests;
