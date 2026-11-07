//! In-process port into the local API that serves Hub business sessions.
//!
//! The API crate implements [`LocalApi`]; the daemon attaches it to the
//! connector, so a session never dials a loopback WebSocket.

use model::share::Visitor;
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

/// The local API refused the connection: draining, at capacity, or a guest
/// whose share no longer admits it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("local API refused the session")]
pub struct Refused;

/// Opens business connections inside this process with the same admission as
/// an authenticated local WebSocket upgrade.
#[async_trait::async_trait]
pub trait LocalApi: std::fmt::Debug + Send + Sync {
    /// Open one connection as the Host owner, or as `guest` confined to one share.
    ///
    /// # Errors
    /// Returns [`Refused`] when the API does not admit the connection.
    async fn connect(&self, guest: Option<&Visitor>) -> Result<Connection, Refused>;
}
