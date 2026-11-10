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

/// Opens business connections inside this process with the same admission as
/// an authenticated local WebSocket upgrade.
#[async_trait::async_trait]
pub trait LocalApi: std::fmt::Debug + Send + Sync {
    /// Open one connection with the authority `admission` names.
    ///
    /// # Errors
    /// Returns [`Refused`] when the API does not admit the connection.
    async fn connect(&self, admission: Admission<'_>) -> Result<Connection, Refused>;
}
