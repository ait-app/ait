//! Calls between sibling Hosts of one account. Discovery and transport remain adapters.

use std::fmt;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

/// Connection state of a Host as the account center last saw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostState {
    /// Its serve link is attached.
    Online,
    /// Its serve link lost its connection and may still resume.
    Reconnecting,
    /// It has no serve link.
    Offline,
}

/// One Host of the account, this one included.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Host {
    /// The daemon's `serverId`, unique within the account; callers address Hosts by it.
    pub server_id: Uuid,
    /// The account's identifier of the Host.
    pub host_id: Uuid,
    /// The node a call is sent to.
    pub node_id: Uuid,
    /// Display name.
    pub name: String,
    /// Machine hostname.
    pub hostname: String,
    /// Operating system platform.
    pub platform: String,
    /// Version of the Host's application.
    pub app_version: String,
    /// Connection state.
    pub state: HostState,
    /// Whether its current link can receive calls from other Hosts.
    pub supported: bool,
    /// Whether it last announced that it accepts calls from other Hosts.
    pub enabled: bool,
}

/// The calling Host, as filled in by the center from its authenticated session.
/// Its serde shape is the center's `open.peer` object (hub-protocol §11.8).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Identity {
    /// The account's identifier of the calling Host.
    pub host_id: Uuid,
    /// The calling daemon's `serverId`.
    pub server_id: Uuid,
    /// Display name of the calling Host.
    pub name: String,
}

/// One tool call forwarded to another Host.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    /// Tool name.
    pub tool: String,
    /// Tool arguments, without the Host selector.
    pub arguments: Value,
    /// Id of the Agent on the calling Host that made the call, when known.
    pub caller: Option<String>,
}

/// Why a call to another Host failed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// This Host has no link to the center, or the center cannot reach other Hosts.
    #[error("account hub unavailable")]
    Unavailable,
    /// No Host of the account has the requested id.
    #[error("unknown host")]
    UnknownHost,
    /// The target Host is not connected.
    #[error("host offline")]
    Offline,
    /// The target Host does not accept calls from other Hosts.
    #[error("host does not accept calls from other hosts")]
    NotAccepting,
    /// The target Host's version cannot receive calls from other Hosts.
    #[error("host does not support calls from other hosts")]
    Unsupported,
    /// Too many calls to other Hosts are open; the call was not sent and may succeed later.
    #[error("too many calls to other hosts")]
    Capacity,
    /// The request exceeds the message size limit or the target's window, so it was not sent;
    /// it fails the same way every time.
    #[error("request too large")]
    RequestTooLarge,
    /// The target ran the call, but its reply exceeds the message size limit and was replaced
    /// with an error.
    #[error("reply too large")]
    ReplyTooLarge,
    /// The request was sent but no reply arrived; it may or may not have run.
    #[error("call outcome unknown")]
    OutcomeUnknown,
    /// The target Host refused the call itself with a JSON-RPC style error,
    /// such as `-32602` for invalid arguments.
    #[error("host rejected the call ({code}): {message}")]
    Rejected {
        /// The error code the target Host replied with.
        code: i32,
        /// The target Host's message, shown to the caller as given.
        message: String,
    },
}

/// Sibling Hosts of this account, reached through the account center.
#[async_trait::async_trait]
pub trait Peers: fmt::Debug + Send + Sync {
    /// Every Host of the account.
    ///
    /// # Errors
    /// Returns `Error::Unavailable` without a center link, or when the center
    /// cannot list Hosts.
    async fn hosts(&self) -> Result<Vec<Host>, Error>;

    /// Send one tool call to the Host on `node`, giving up after `timeout`.
    /// Dropping the future also gives up the call; a call given up before
    /// its request was sent never reaches the target.
    ///
    /// # Arguments
    /// * `node` - The node of the target Host.
    /// * `request` - The tool call.
    /// * `timeout` - How long the caller waits, from the call until the reply.
    ///
    /// # Returns
    /// The `result` object the target Host replied with.
    ///
    /// # Errors
    /// Returns the reason the call did not reach the target, among them
    /// `Error::RequestTooLarge` for a request over the message size limit or
    /// the target's window; `Error::OutcomeUnknown` when it was sent but no
    /// reply arrived; and the target's answer (`Error::NotAccepting`,
    /// `Error::ReplyTooLarge` for a call that ran but whose reply was too
    /// large, or `Error::Rejected`) when it replied with an error. When `timeout` passes
    /// the error is `Error::Offline` if the target had not accepted the call's
    /// session and `Error::OutcomeUnknown` if the request was sent. A call
    /// still waiting for its turn to be sent ends as `Error::Unavailable` when
    /// the link is not connected, and as `Error::Capacity` only while it is.
    async fn call(&self, node: Uuid, request: Request, timeout: Duration) -> Result<Value, Error>;
}

#[cfg(test)]
mod tests;
