//! Messages of `k:"peer"` sessions between sibling Hosts of one account
//! (`ait-hub-v1` feature `peer`), in both directions.
//!
//! The `open` and `accept` payloads declare the version (`{"v":1}`). Every later
//! message is one UTF-8 JSON text: a request
//! `{"id","tool","arguments","caller"?:{"agentId"}}`, or its reply
//! `{"id","result"}` / `{"id","error":{"code","message"}}`.

use model::peer::Request;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio_tungstenite::tungstenite::Bytes;

use super::bridge::PeerRefusal;
use super::wire;

/// Message version of peer sessions.
pub(super) const VERSION: u64 = 1;
/// Payload of a peer `open` and `accept`.
pub(super) const HELLO: &[u8] = br#"{"v":1}"#;
/// Error code of the replies this Host substitutes: a refusal because it does
/// not accept calls, and a reply over the payload limit.
const SERVER_ERROR: i32 = -32_000;
/// Message of the error that replaces a reply over the payload limit.
const TOO_LARGE: &str = "reply exceeds the message size limit";

/// A peer message that is not what the protocol defines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("malformed peer message")]
pub(super) struct Malformed;

/// The version declaration of an `open` or `accept` payload.
#[derive(Deserialize)]
struct Hello {
    v: u64,
}

#[derive(Deserialize)]
struct WireRequest {
    id: u64,
    tool: String,
    arguments: Map<String, Value>,
    #[serde(default)]
    caller: Option<Caller>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Caller {
    agent_id: String,
}

#[derive(Serialize)]
struct Reply {
    id: u64,
    #[serde(flatten)]
    outcome: Outcome,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum Outcome {
    Result(Value),
    Error { code: i32, message: String },
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
    Ok((
        request.id,
        Request {
            tool: request.tool,
            arguments: Value::Object(request.arguments),
            caller: request.caller.map(|caller| caller.agent_id),
        },
    ))
}

/// Encode the reply to request `id`: its result object, or an error code and
/// message. A reply over the payload limit is replaced by an error.
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
        code: SERVER_ERROR,
        message: TOO_LARGE.to_owned(),
    };
    serialize(&Reply { id, outcome }).into()
}

/// The error a refused call is answered with.
pub(super) fn refusal(refusal: PeerRefusal) -> (i32, String) {
    match refusal {
        PeerRefusal::Disabled => (SERVER_ERROR, refusal.to_string()),
        PeerRefusal::Failed { code, message } => (code, message),
    }
}

fn serialize(reply: &Reply) -> Vec<u8> {
    serde_json::to_vec(reply).expect("a reply of JSON values always serializes")
}

#[cfg(test)]
mod tests;
