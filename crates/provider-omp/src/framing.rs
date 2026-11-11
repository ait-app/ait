//! OMP stdout framing: physical JSONL lines become routable frames, with protocol v2 chunk
//! reassembly. The tolerance and limits follow Paseo's `jsonl-frame-decoder.ts`.

use base64::Engine;
use serde_json::Value;

/// Largest physical stdout line, excluding its newline; a longer line closes the connection.
pub(super) const MAX_LINE: usize = 64 * 1024 * 1024;
/// Physical frame size OMP must advertise before protocol v2 is negotiated.
const V2_FRAME_BYTES: usize = 1024 * 1024;
/// Largest reassembled logical frame, which OMP must also advertise.
const V2_REASSEMBLED_BYTES: usize = 64 * 1024 * 1024;
const V2_CHUNK_BYTES: usize = 256 * 1024;
const V2_BASE64_CHARS: usize = V2_CHUNK_BYTES.div_ceil(3) * 4;
const V2_MAX_CHUNKS: usize = V2_REASSEMBLED_BYTES.div_ceil(V2_CHUNK_BYTES);
/// Longest chunk id, in UTF-16 code units as JavaScript counts string length.
const MAX_CHUNK_ID: usize = 128;

/// Why one physical line produced no frame. Every problem is logged and the line dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Problem {
    /// The line is not JSON.
    InvalidJson,
    /// The line is JSON but not an object.
    NotObject,
    /// The object has no string `type` to route it by.
    MissingType,
    /// A chunk header is missing a field or exceeds a protocol v2 bound.
    InvalidChunk,
    /// Chunk data is not canonical base64 within the per-chunk size.
    InvalidBase64,
    /// A chunk does not continue the current sequence.
    OutOfOrderChunk,
    /// The chunk bytes do not add up to the declared length.
    ChunkLengthMismatch,
    /// The reassembled bytes are not a strict UTF-8 JSON object with a routable type.
    InvalidChunkPayload,
}

/// One logical frame and the bytes it occupied, counted against buffer limits.
#[derive(Debug, PartialEq)]
pub(super) struct Frame {
    pub(super) value: Value,
    pub(super) bytes: usize,
}

#[derive(Debug)]
struct Assembly {
    id: String,
    count: usize,
    length: usize,
    parts: usize,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
struct Header<'a> {
    id: &'a str,
    index: usize,
    count: usize,
    length: usize,
    data: &'a str,
}

/// Stateful decoder for one stdout stream; at most one chunk sequence is open at a time.
#[derive(Debug, Default)]
pub(super) struct Decoder(Option<Assembly>);

impl Decoder {
    /// Decode one physical line received without its newline.
    ///
    /// # Returns
    ///
    /// The routable frame, or `None` for a blank line or an unfinished chunk sequence.
    /// Frames between chunks pass through without disturbing the open sequence.
    ///
    /// # Errors
    ///
    /// Returns the [`Problem`] that made the line unusable. Invalid chunks discard the open
    /// sequence, except a stray continuation that arrives while none is open.
    pub(super) fn line(&mut self, line: &[u8]) -> Result<Option<Frame>, Problem> {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.iter().all(u8::is_ascii_whitespace) {
            return Ok(None);
        }
        let value: Value = serde_json::from_slice(line).map_err(|_| Problem::InvalidJson)?;
        if !value.is_object() {
            return Err(Problem::NotObject);
        }
        if value["type"] == "rpc_chunk" {
            return self.chunk(&value);
        }
        if !value["type"].is_string() {
            return Err(Problem::MissingType);
        }
        Ok(Some(Frame {
            value,
            bytes: line.len(),
        }))
    }

    fn chunk(&mut self, frame: &Value) -> Result<Option<Frame>, Problem> {
        let Some(header) = header(frame) else {
            return Err(self.discard(Problem::InvalidChunk));
        };
        let Some(bytes) = chunk_bytes(header.data) else {
            return Err(self.discard(Problem::InvalidBase64));
        };
        if self.0.is_none() && header.index != 0 {
            return Err(Problem::OutOfOrderChunk);
        }
        let assembly = self.0.get_or_insert_with(|| Assembly {
            id: header.id.to_owned(),
            count: header.count,
            length: header.length,
            parts: 0,
            bytes: Vec::with_capacity(header.length),
        });
        if assembly.id != header.id
            || assembly.count != header.count
            || assembly.length != header.length
            || assembly.parts != header.index
        {
            return Err(self.discard(Problem::OutOfOrderChunk));
        }
        if assembly.bytes.len() + bytes.len() > assembly.length {
            return Err(self.discard(Problem::ChunkLengthMismatch));
        }
        assembly.bytes.extend_from_slice(&bytes);
        assembly.parts += 1;
        if assembly.parts < assembly.count {
            return Ok(None);
        }
        let Some(assembly) = self.0.take() else {
            return Err(Problem::OutOfOrderChunk);
        };
        if assembly.bytes.len() != assembly.length {
            return Err(Problem::ChunkLengthMismatch);
        }
        // serde_json rejects invalid UTF-8 as strictly as a fatal TextDecoder.
        let value: Value =
            serde_json::from_slice(&assembly.bytes).map_err(|_| Problem::InvalidChunkPayload)?;
        if !value["type"].is_string() || value["type"] == "rpc_chunk" {
            return Err(Problem::InvalidChunkPayload);
        }
        Ok(Some(Frame {
            value,
            bytes: assembly.length,
        }))
    }

    fn discard(&mut self, problem: Problem) -> Problem {
        self.0 = None;
        problem
    }
}

/// Report whether OMP's `ready` frame offers protocol v2 with the frame sizes Ait accepts.
/// Any other `ready` keeps the connection on protocol v1 without negotiation.
pub(super) fn supports_v2(ready: &Value) -> bool {
    ready["type"] == "ready"
        && ready["supportedProtocolVersions"]
            .as_array()
            .is_some_and(|versions| versions.iter().any(|version| *version == 2))
        && ready["maxFrameBytes"] == V2_FRAME_BYTES
        && ready["maxReassembledFrameBytes"] == V2_REASSEMBLED_BYTES
}

fn header(frame: &Value) -> Option<Header<'_>> {
    let number = |key: &str| {
        frame[key]
            .as_u64()
            .and_then(|number| usize::try_from(number).ok())
    };
    let header = Header {
        id: frame["chunkId"]
            .as_str()
            .filter(|id| !id.is_empty() && id.encode_utf16().nth(MAX_CHUNK_ID).is_none())?,
        index: number("index")?,
        count: number("count")?,
        length: number("byteLength")?,
        data: frame["data"].as_str().filter(|data| !data.is_empty())?,
    };
    ((2..=V2_MAX_CHUNKS).contains(&header.count)
        && header.index < header.count
        && (V2_FRAME_BYTES..=V2_REASSEMBLED_BYTES).contains(&header.length))
    .then_some(header)
}

fn chunk_bytes(data: &str) -> Option<Vec<u8>> {
    if data.len() > V2_BASE64_CHARS {
        return None;
    }
    // STANDARD requires canonical padding and zero trailing bits, so every decodable string
    // equals its own re-encoding, which is the canonical check Paseo performs.
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .ok()?;
    (bytes.len() <= V2_CHUNK_BYTES).then_some(bytes)
}

#[cfg(test)]
mod tests;
