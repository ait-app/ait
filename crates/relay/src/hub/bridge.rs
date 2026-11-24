//! In-process port into the local API that serves Hub business sessions.
//!
//! The API crate implements [`LocalApi`]; the daemon attaches it to the
//! connector, so a session never dials a loopback WebSocket.

use model::peer::{Identity, Request};
use model::share::Visitor;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Bytes;

use super::wire::PayloadKind;

/// One business WebSocket message between a Hub session and the local API.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    /// A UTF-8 text message.
    Text(Bytes),
    /// A binary message.
    Binary(Bytes),
}

impl Frame {
    /// Split into the wire payload kind and its bytes.
    pub(super) fn into_parts(self) -> (PayloadKind, Bytes) {
        match self {
            Self::Text(bytes) => (PayloadKind::Text, bytes),
            Self::Binary(bytes) => (PayloadKind::Binary, bytes),
        }
    }

    /// Payload bytes, as counted by session flow control.
    pub(super) fn len(&self) -> usize {
        match self {
            Self::Text(bytes) | Self::Binary(bytes) => bytes.len(),
        }
    }
}

/// Both directions of one in-process business connection. Dropping `sender`
/// ends the connection; the API drops its end of `receiver` when it closes.
#[derive(Debug)]
pub struct Connection {
    /// Messages for the local API, starting with the client hello.
    pub sender: mpsc::Sender<Frame>,
    /// Messages from the local API, starting with its first reply.
    pub receiver: mpsc::Receiver<Frame>,
}

/// Who one in-process business connection runs as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission<'a> {
    /// The Host owner, with the API's full authority.
    Owner,
    /// A visiting account confined to one share.
    Guest(&'a Visitor),
    /// The center's automation session, confined to the local automation policy.
    Automation,
}

/// The local API refused the connection: draining, at capacity, a guest whose
/// share no longer admits it, or automation while it is disabled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("local API refused the session")]
pub struct Refused;

/// Why the local API did not run a call from another Host.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PeerRefusal {
    /// This Host does not accept calls from other Hosts; the session ends.
    #[error("this host does not accept calls from other hosts")]
    Disabled,
    /// The call was refused or failed; the caller gets `code` and `message`.
    #[error("{message}")]
    Failed {
        /// JSON-RPC style error code.
        code: i32,
        /// Human-readable reason.
        message: String,
    },
}

/// Opens business connections inside this process with the same admission as
/// an authenticated local WebSocket upgrade, and runs calls from other Hosts.
#[async_trait::async_trait]
pub trait LocalApi: std::fmt::Debug + Send + Sync {
    /// Open one connection with the authority `admission` names.
    ///
    /// # Errors
    /// Returns [`Refused`] when the API does not admit the connection.
    async fn connect(&self, admission: Admission<'_>) -> Result<Connection, Refused>;

    /// Run one tool call that the Host `from` sent over a peer session.
    ///
    /// # Arguments
    /// * `from` - The calling Host, as the center authenticated it.
    /// * `request` - The tool call.
    ///
    /// # Returns
    /// The tool call's result object.
    ///
    /// # Errors
    /// Returns [`PeerRefusal::Disabled`] when this Host does not accept calls
    /// from other Hosts (the default), and [`PeerRefusal::Failed`] when the
    /// call is refused or fails.
    async fn peer(&self, _from: &Identity, _request: Request) -> Result<Value, PeerRefusal> {
        Err(PeerRefusal::Disabled)
    }
}

#[cfg(test)]
mod tests;
