//! Relay wire contracts. This module performs no network or filesystem I/O.

use secrecy::SecretString;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Error;

/// A short-lived credential handed over by the trusted desktop account manager.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlGrant {
    /// HTTPS center base URL; HTTP is accepted only for loopback development.
    pub(crate) center_url: String,
    /// One-use control ticket, never a user JWT.
    pub(crate) control_ticket: String,
    /// Node activation associated with this ticket.
    pub(crate) node_session_id: Uuid,
}

/// Non-secret connector state exposed to the account manager.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Status {
    /// Whether a control attempt is still in progress.
    pub connecting: bool,
    /// Whether the center accepted this host's control hello.
    pub(crate) online: bool,
    /// Last accepted routing generation, used only to replace the same instance.
    pub(crate) epoch: Option<Uuid>,
    /// Non-secret machine-readable failure category.
    pub(crate) error: Option<String>,
}

/// Initial declaration of the local daemon's identity to the center.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename = "control.hello")]
pub(super) struct ControlHello<'a> {
    version: u8,
    node_session_id: Uuid,
    server_id: &'a str,
    instance_id: &'a str,
    resume_epoch: Option<Uuid>,
}

impl<'a> ControlHello<'a> {
    /// Construct a version-one hello for the supplied node, daemon and optional previous epoch.
    pub(super) fn new(
        node_session_id: Uuid,
        server_id: &'a str,
        instance_id: &'a str,
        resume_epoch: Option<Uuid>,
    ) -> Self {
        Self {
            version: 1,
            node_session_id,
            server_id,
            instance_id,
            resume_epoch,
        }
    }
}

/// The only accepted response to the initial control hello.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(super) enum ControlWelcome {
    /// Accepted routing generation for subsequent commands.
    #[serde(rename = "control.welcome")]
    Welcome { epoch: Uuid },
}

/// Commands admitted after the control handshake.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(super) enum ControlCommand {
    /// Open one independent reverse data connection.
    #[serde(rename = "open_data")]
    OpenData(OpenData),
    /// Cancel an existing reverse data connection.
    #[serde(rename = "cancel_session")]
    CancelSession { relay_session_id: Uuid },
}

/// A data grant whose mode determines which additional credentials are required.
#[derive(Debug, Deserialize)]
pub(super) struct OpenData {
    /// Connection identity assigned by the center.
    pub(crate) relay_session_id: Uuid,
    /// Control generation that authorized this grant.
    pub(crate) epoch: Uuid,
    /// One-use credential for the daemon side of the data connection.
    pub(crate) daemon_ticket: SecretString,
    /// Business forwarding or a scoped file download.
    #[serde(flatten)]
    pub(crate) mode: DataMode,
}

/// Supported data modes and their mode-specific parameters.
#[derive(Debug, Deserialize)]
#[serde(tag = "mode")]
pub(super) enum DataMode {
    /// A transparent connection carrying the daemon's existing business protocol.
    #[serde(rename = "ait-rust-single-v1")]
    RustSingle,
    /// An independent download authorized by a local one-use file token.
    #[serde(rename = "ait-download-v1")]
    Download { download_token: SecretString },
}

/// Pairing acknowledgement for one specific reverse data connection.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(super) enum Pairing {
    /// Both endpoints joined the named session.
    #[serde(rename = "relay.ready")]
    Ready { relay_session_id: Uuid },
}

impl Pairing {
    /// Verify that the acknowledgement belongs to the expected data session.
    ///
    /// # Errors
    /// Rejects acknowledgements for any other session.
    pub(super) fn verify(self, expected: Uuid) -> Result<(), Error> {
        let Self::Ready { relay_session_id } = self;
        if relay_session_id != expected {
            return Err(Error::Protocol);
        }
        Ok(())
    }
}

/// Only the envelope is inspected; the local daemon owns the business hello's fields.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(super) enum ClientHello {
    /// A client hello whose original JSON is forwarded without reconstruction.
    #[serde(rename = "hello")]
    Hello,
}

/// Text frames surrounding the binary body of an independent download.
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub(super) enum DownloadMessage<'a> {
    /// Metadata returned by the local file endpoint.
    #[serde(rename = "download.headers")]
    Headers {
        status: u16,
        content_length: Option<u64>,
        content_type: Option<&'a str>,
    },
    /// Number of body bytes successfully sent before waiting for the destination.
    #[serde(rename = "download.end")]
    End { bytes: u64 },
}

/// Destination acknowledgement after the final write and file synchronization.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(super) enum DownloadAck {
    /// All advertised bytes have been received and synchronized.
    #[serde(rename = "download.complete")]
    Complete,
}

/// Decode one JSON message without including untrusted payloads in errors.
///
/// # Errors
/// Rejects malformed JSON, unknown message kinds and missing or invalid required fields.
pub(super) fn decode<T: DeserializeOwned>(text: &str) -> Result<T, Error> {
    serde_json::from_str(text).map_err(|_| Error::Protocol)
}

/// Encode one typed message using the existing JSON wire representation.
///
/// # Errors
/// Returns a non-secret protocol error if serialization fails.
pub(super) fn encode(message: &impl Serialize) -> Result<String, Error> {
    serde_json::to_string(message).map_err(|_| Error::Protocol)
}

/// Validate the bounded token syntax accepted by the local download endpoint.
///
/// # Errors
/// Rejects empty tokens, tokens over 128 bytes and characters other than ASCII alphanumerics or '-'.
pub(super) fn validate_download_token(token: &str) -> Result<(), Error> {
    if token.is_empty()
        || token.len() > 128
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(Error::Protocol);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
