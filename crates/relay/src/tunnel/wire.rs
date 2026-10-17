//! Version-one tunnel records; kept wire-compatible with the center implementation.

use std::io;

use futures_util::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub(super) const PROTOCOL: &str = "ait-relay-tunnel-v1";
pub(super) const MAX_BLOCK: usize = 64 * 1024;
pub(super) const MAX_BODY: usize = 1024 * 1024;
pub(super) const OPEN: u8 = 1;
pub(super) const ACCEPT: u8 = 2;
pub(super) const START: u8 = 3;
pub(super) const TEXT: u8 = 4;
pub(super) const BINARY: u8 = 5;
pub(super) const CLOSE: u8 = 6;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// First text message and echoed welcome, binding this socket to one control generation.
pub(super) struct Prelude {
    pub(super) protocol: String,
    pub(super) epoch: Uuid,
    pub(super) tunnel_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Center-authorized visit; deliberately contains no destination URL or local credential.
pub(super) struct Open {
    pub(super) relay_session_id: Uuid,
    pub(super) source_node_session_id: Uuid,
    pub(super) epoch: Uuid,
    pub(super) tunnel_id: Uuid,
    pub(super) mode: String,
}

/// Return a protocol error without including peer payloads or credentials.
pub(super) fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Invalid relay tunnel data")
}

fn limit(kind: u8) -> io::Result<usize> {
    match kind {
        OPEN => Ok(2048),
        ACCEPT | START | CLOSE => Ok(0),
        TEXT | BINARY => Ok(MAX_BODY),
        _ => Err(invalid()),
    }
}

/// Read one bounded record, consuming stream credit incrementally.
///
/// Returns an I/O error for truncated, unknown, oversized or non-UTF-8 text records.
pub(super) async fn read<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<(u8, Vec<u8>)> {
    let mut header = [0; 5];
    reader.read_exact(&mut header).await?;
    let length = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
    if length > limit(header[0])? {
        return Err(invalid());
    }
    // The caller reserves the maximum body budget before admitting a stream.
    // Incremental reads release Yamux credit even for bodies larger than its window.
    let mut body = vec![0; length];
    reader.read_exact(&mut body).await?;
    if header[0] == TEXT && std::str::from_utf8(&body).is_err() {
        return Err(invalid());
    }
    Ok((header[0], body))
}

/// Write and flush one record; reject unknown kinds and oversized bodies before writing.
pub(super) async fn write<W: AsyncWrite + Unpin>(
    writer: &mut W,
    kind: u8,
    body: &[u8],
) -> io::Result<()> {
    if body.len() > limit(kind)? {
        return Err(invalid());
    }
    let length = u32::try_from(body.len()).map_err(|_| invalid())?;
    writer.write_all(&[kind]).await?;
    writer.write_all(&length.to_be_bytes()).await?;
    writer.write_all(body).await?;
    writer.flush().await
}

/// Consume the next empty control record, rejecting a mismatched kind or failed read.
pub(super) async fn expect<R: AsyncRead + Unpin>(reader: &mut R, kind: u8) -> io::Result<()> {
    let (received, _) = read(reader).await?;
    if received != kind {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
#[path = "wire/tests.rs"]
mod tests;
