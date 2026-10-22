//! Binary envelope for the center's ait-hub-v1 protocol.

use serde::{Deserialize, Serialize};
use tokio_tungstenite::tungstenite::{Bytes, Message};
use uuid::Uuid;

use crate::Error;

pub(super) const MAX_HEADER: usize = 4096;
pub(super) const MAX_PAYLOAD: usize = 1024 * 1024;
pub(super) const MAX_FRAME: usize = 2 + MAX_HEADER + MAX_PAYLOAD;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub(super) struct Resume {
    pub link: Uuid,
    pub recv: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum PayloadKind {
    Text,
    Binary,
}

#[derive(Debug, Serialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub(super) enum Outbound {
    Hello {
        v: u8,
        resume: Option<Resume>,
    },
    Ack {
        a: u64,
    },
    Accept {
        seq: u64,
        s: Uuid,
    },
    Msg {
        seq: u64,
        s: Uuid,
        pt: PayloadKind,
    },
    Close {
        seq: u64,
        s: Uuid,
        code: &'static str,
    },
}

#[derive(Debug, Deserialize)]
pub(super) struct Welcome {
    pub v: u8,
    pub link: Uuid,
    pub node_id: Uuid,
    pub resumed: bool,
    pub recv: u64,
    pub resume_token: Option<secrecy::SecretString>,
    pub grace_seconds: u64,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub(super) enum Inbound {
    Welcome(Welcome),
    Ack { a: u64 },
    Error { code: String },
    Open { seq: u64, s: Uuid, from: Uuid },
    Msg { seq: u64, s: Uuid, pt: PayloadKind },
    Close { seq: u64, s: Uuid },
}

pub(super) fn decode(bytes: &Bytes) -> Result<(Inbound, Bytes), Error> {
    let prefix: [u8; 2] = bytes
        .get(..2)
        .ok_or(Error::Protocol)?
        .try_into()
        .map_err(|_| Error::Protocol)?;
    let length = usize::from(u16::from_be_bytes(prefix));
    if length > MAX_HEADER || bytes.len() < 2 + length {
        return Err(Error::Protocol);
    }
    let payload = bytes.slice(2 + length..);
    if payload.len() > MAX_PAYLOAD {
        return Err(Error::Protocol);
    }
    let header = serde_json::from_slice(&bytes[2..2 + length]).map_err(|_| Error::Protocol)?;
    Ok((header, payload))
}

pub(super) fn encode(header: &Outbound, payload: &[u8]) -> Result<Bytes, Error> {
    let header = serde_json::to_vec(header).map_err(|_| Error::Protocol)?;
    if header.len() > MAX_HEADER || payload.len() > MAX_PAYLOAD {
        return Err(Error::Protocol);
    }
    let length = u16::try_from(header.len()).map_err(|_| Error::Protocol)?;
    let mut bytes = Vec::with_capacity(2 + header.len() + payload.len());
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(&header);
    bytes.extend_from_slice(payload);
    Ok(bytes.into())
}

pub(super) fn message(kind: PayloadKind, payload: Bytes) -> Result<Message, Error> {
    match kind {
        PayloadKind::Text => String::from_utf8(payload.to_vec())
            .map(|text| Message::Text(text.into()))
            .map_err(|_| Error::Protocol),
        PayloadKind::Binary => Ok(Message::Binary(payload)),
    }
}

#[cfg(test)]
mod tests;
