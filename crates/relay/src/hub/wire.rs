//! `ait-hub-v1` frames as seen by a serve endpoint; wire-compatible with the center.
//!
//! Each frame is one WebSocket Binary message: `header_len:u16 BE | JSON header | payload`.

use model::share::Visitor;
use serde::{Deserialize, Serialize};
use tokio_tungstenite::tungstenite::Bytes;
use uuid::Uuid;

use crate::Error;

pub(super) const VERSION: u32 = 1;
const MAX_HEADER: usize = 4 * 1024;
pub(super) const MAX_PAYLOAD: usize = 1024 * 1024;
pub(super) const MAX_FRAME: usize = 2 + MAX_HEADER + MAX_PAYLOAD;
/// Optional link features this serve endpoint implements; `hello.f` offers the
/// subset the center declared over HTTP.
pub(super) const FEATURES: [&str; 3] = ["lease", "credit", "download"];

/// Features to offer in `hello.f`: the center's declared set intersected with
/// [`FEATURES`]. `None` (a center predating features) keeps the plain hello.
pub(super) fn offer(declared: Option<&[String]>) -> Option<Vec<&'static str>> {
    declared.map(|declared| {
        FEATURES
            .into_iter()
            .filter(|feature| declared.iter().any(|name| name == feature))
            .collect()
    })
}

/// Business WebSocket message type carried in a payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum PayloadKind {
    Text,
    Binary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(super) struct Resume {
    pub(super) link: Uuid,
    pub(super) recv: u64,
}

/// What an opened session carries; omitted on the wire for business sessions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum SessionKind {
    #[default]
    Rpc,
    Download,
    /// A kind this Host does not implement; refused as `unsupported`.
    #[serde(other)]
    Unsupported,
}

/// Frames a serve endpoint sends. Sequenced kinds carry `seq`.
#[derive(Debug, Serialize)]
#[serde(tag = "t")]
pub(super) enum Outbound {
    /// `f` is present only when the center declared features over HTTP.
    #[serde(rename = "hello")]
    Hello {
        v: u32,
        resume: Option<Resume>,
        #[serde(skip_serializing_if = "Option::is_none")]
        f: Option<Vec<&'static str>>,
    },
    #[serde(rename = "ack")]
    Ack { a: u64 },
    #[serde(rename = "call")]
    Call {
        seq: u64,
        id: Uuid,
        method: &'static str,
    },
    /// `w` (this Host's receive window) only answers an `open` that carried one.
    #[serde(rename = "accept")]
    Accept {
        seq: u64,
        s: Uuid,
        #[serde(skip_serializing_if = "Option::is_none")]
        w: Option<u32>,
    },
    #[serde(rename = "msg")]
    Msg { seq: u64, s: Uuid, pt: PayloadKind },
    #[serde(rename = "close")]
    Close {
        seq: u64,
        s: Uuid,
        code: &'static str,
    },
    /// Cumulative payload bytes of session `s` delivered locally; unsequenced.
    #[serde(rename = "credit")]
    Credit { s: Uuid, c: u64 },
}

/// Frames the center sends to a serve endpoint. Unknown fields are tolerated
/// so the center can extend headers without breaking deployed Hosts.
#[derive(Debug, Deserialize)]
#[serde(tag = "t")]
pub(super) enum Inbound {
    #[serde(rename = "welcome")]
    Welcome {
        link: Uuid,
        resumed: bool,
        recv: u64,
        resume_token: Option<String>,
        grace_seconds: u64,
        #[serde(default)]
        center_methods: Vec<String>,
        /// Features enabled on this link; present only when the hello offered `f`.
        #[serde(default)]
        f: Vec<String>,
    },
    #[serde(rename = "ack")]
    Ack { a: u64 },
    #[serde(rename = "reply")]
    Reply { seq: u64, id: Uuid },
    #[serde(rename = "error")]
    Error { code: String },
    /// `visitor` marks a guest session confined to one shared resource; `w`, the
    /// opener's receive window, is present only on flow-controlled sessions.
    #[serde(rename = "open")]
    Open {
        seq: u64,
        s: Uuid,
        #[serde(default)]
        visitor: Option<Visitor>,
        #[serde(default)]
        k: SessionKind,
        #[serde(default)]
        w: Option<u32>,
    },
    #[serde(rename = "msg")]
    Msg { seq: u64, s: Uuid, pt: PayloadKind },
    #[serde(rename = "close")]
    Close { seq: u64, s: Uuid },
    /// Center-initiated outcome of the published shares; sent only after this
    /// Host called `hub.shares.publish` on the link.
    #[serde(rename = "share_status")]
    ShareStatus { seq: u64 },
    /// The peer's cumulative delivered bytes of a flow-controlled session.
    #[serde(rename = "credit")]
    Credit { s: Uuid, c: u64 },
}

impl Inbound {
    /// Sequence number of a sequenced frame.
    pub(super) const fn seq(&self) -> Option<u64> {
        match self {
            Self::Open { seq, .. }
            | Self::Msg { seq, .. }
            | Self::Close { seq, .. }
            | Self::Reply { seq, .. }
            | Self::ShareStatus { seq } => Some(*seq),
            Self::Welcome { .. } | Self::Ack { .. } | Self::Error { .. } | Self::Credit { .. } => {
                None
            }
        }
    }
}

/// Encode one frame.
///
/// # Errors
/// Returns `Error::Protocol` when the payload exceeds the protocol limit.
pub(super) fn encode(header: &Outbound, payload: &[u8]) -> Result<Bytes, Error> {
    if payload.len() > MAX_PAYLOAD {
        return Err(Error::Protocol);
    }
    let header = serde_json::to_vec(header).map_err(|_| Error::Protocol)?;
    let length = u16::try_from(header.len()).map_err(|_| Error::Protocol)?;
    let mut frame = Vec::with_capacity(2 + header.len() + payload.len());
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(&header);
    frame.extend_from_slice(payload);
    Ok(frame.into())
}

/// Split one frame into its header and payload without copying the payload.
///
/// # Errors
/// Returns `Error::Protocol` for truncated, oversized or unrecognized frames.
pub(super) fn decode(frame: &Bytes) -> Result<(Inbound, Bytes), Error> {
    let [high, low, ..] = frame.as_ref() else {
        return Err(Error::Protocol);
    };
    let length = usize::from(u16::from_be_bytes([*high, *low]));
    if length > MAX_HEADER || frame.len() < 2 + length || frame.len() - 2 - length > MAX_PAYLOAD {
        return Err(Error::Protocol);
    }
    let header = serde_json::from_slice(&frame[2..2 + length]).map_err(|_| Error::Protocol)?;
    Ok((header, frame.slice(2 + length..)))
}

#[cfg(test)]
mod tests;
