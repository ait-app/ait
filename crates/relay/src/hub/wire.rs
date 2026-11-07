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
/// Link features of a main serve link. The center must declare every one of
/// them (`hub_features`); this daemon does not serve centers predating them.
pub(super) const FEATURES: [&str; 4] = ["lease", "credit", "download", BULK];
/// Feature letting the center ask the main link for a download-only bulk link.
pub(super) const BULK: &str = "bulk";
/// The whole `hello.f` of a bulk link; the center requires both.
pub(super) const BULK_FEATURES: [&str; 2] = ["credit", "download"];

/// `hello.f` of a main link. `bulk` is declared only when the owner can obtain
/// bulk tickets (`bulk_lane`), since the link must open a bulk link on request.
pub(super) fn offer(bulk_lane: bool) -> Vec<&'static str> {
    FEATURES
        .into_iter()
        .filter(|feature| bulk_lane || *feature != BULK)
        .collect()
}

/// The first feature of `offer` missing from `enabled`, if any.
pub(super) fn missing(offer: &[&'static str], enabled: &[String]) -> Option<&'static str> {
    offer
        .iter()
        .copied()
        .find(|feature| !enabled.iter().any(|name| name == feature))
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
    /// `f` is the same on every connection of one link.
    #[serde(rename = "hello")]
    Hello {
        v: u32,
        resume: Option<Resume>,
        f: Vec<&'static str>,
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
        /// Features enabled on this link; absent only from centers predating
        /// them, which this daemon refuses.
        #[serde(default)]
        f: Vec<String>,
    },
    #[serde(rename = "ack")]
    Ack { a: u64 },
    #[serde(rename = "reply")]
    Reply { seq: u64, id: Uuid },
    #[serde(rename = "error")]
    Error { code: String },
    /// `visitor` marks a guest session confined to one shared resource, and
    /// `service` one the center opened for itself; `w`, the opener's receive
    /// window, is present only on flow-controlled sessions.
    #[serde(rename = "open")]
    Open {
        seq: u64,
        s: Uuid,
        #[serde(default)]
        visitor: Option<Visitor>,
        #[serde(default)]
        service: Option<Service>,
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
    /// Hint to open the link for `lane` (only `bulk` is defined); sent only to
    /// a main link that declared `bulk`.
    #[serde(rename = "lane")]
    Lane { lane: String },
}

/// A center-opened session; only `automation` is defined.
#[derive(Debug, Deserialize)]
pub(super) struct Service {
    /// What the center opened the session for.
    pub(super) kind: String,
}

impl Service {
    /// Whether this is an automation session; other kinds are refused.
    pub(super) fn is_automation(&self) -> bool {
        self.kind == "automation"
    }
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
            Self::Welcome { .. }
            | Self::Ack { .. }
            | Self::Error { .. }
            | Self::Credit { .. }
            | Self::Lane { .. } => None,
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
