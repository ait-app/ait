//! Download and business-hello wire contracts shared with the Hub link. This
//! module performs no network or filesystem I/O.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Error;

/// State of the retired Relay control connection, always offline. It keeps the
/// shape of `relay.status.request` and `GET /api/relay/control` responses, which
/// clients still read for the daemon's identity and management state; the Hub
/// link reports its own state.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Status {
    /// Always `false`: no control connection is attempted.
    pub connecting: bool,
    /// Always `false`: no control connection exists.
    pub online: bool,
    /// Always `None`.
    epoch: Option<Uuid>,
    /// Always `None`.
    error: Option<String>,
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
