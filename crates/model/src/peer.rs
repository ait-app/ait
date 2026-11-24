//! Calls between sibling Hosts of one account. Discovery and transport remain adapters.

use std::fmt;

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
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
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
    /// Too many calls to other Hosts are open.
    #[error("too many calls to other hosts")]
    Capacity,
    /// The request or its reply exceeds the size limit.
    #[error("call too large")]
    TooLarge,
    /// The request was sent but no reply arrived; it may or may not have run.
    #[error("call outcome unknown")]
    OutcomeUnknown,
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

    /// Send one tool call to the Host on `node`.
    ///
    /// # Returns
    /// The `result` object the target Host replied with.
    ///
    /// # Errors
    /// Returns the reason the call did not reach the target, or
    /// `Error::OutcomeUnknown` when it was sent but no reply arrived.
    async fn call(&self, node: Uuid, request: Request) -> Result<Value, Error>;
}
