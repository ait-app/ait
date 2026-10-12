use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use model::ErrorCode;

// Wire schema from the official larksuite/oapi-sdk-go ws/pbbp2.proto frame.
#[derive(Clone, PartialEq, prost::Message)]
pub(super) struct Header {
    #[prost(string, required, tag = "1")]
    pub(super) key: String,
    #[prost(string, required, tag = "2")]
    pub(super) value: String,
}
#[derive(Clone, PartialEq, prost::Message)]
pub(super) struct Frame {
    #[prost(uint64, required, tag = "1")]
    pub(super) seq_id: u64,
    #[prost(uint64, required, tag = "2")]
    pub(super) log_id: u64,
    #[prost(int32, required, tag = "3")]
    pub(super) service: i32,
    #[prost(int32, required, tag = "4")]
    pub(super) method: i32,
    #[prost(message, repeated, tag = "5")]
    pub(super) headers: Vec<Header>,
    #[prost(string, optional, tag = "6")]
    pub(super) payload_encoding: Option<String>,
    #[prost(string, optional, tag = "7")]
    pub(super) payload_type: Option<String>,
    #[prost(bytes = "vec", optional, tag = "8")]
    pub(super) payload: Option<Vec<u8>>,
    #[prost(string, optional, tag = "9")]
    pub(super) log_id_new: Option<String>,
}

impl Frame {
    pub(super) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|header| header.key == name)
            .map(|header| header.value.as_str())
    }
}

struct Parts {
    expires: Instant,
    chunks: Vec<Option<Vec<u8>>>,
    bytes: usize,
}

#[derive(Default)]
pub(super) struct Fragments(BTreeMap<String, Parts>);

impl Fragments {
    pub(super) fn combine(&mut self, frame: &Frame) -> Result<Option<Vec<u8>>, ErrorCode> {
        let payload = frame.payload.as_ref().ok_or(ErrorCode::BotIo)?;
        let sum = frame
            .header("sum")
            .unwrap_or("1")
            .parse::<usize>()
            .map_err(|_| ErrorCode::BotIo)?;
        let sequence = frame
            .header("seq")
            .unwrap_or("0")
            .parse::<usize>()
            .map_err(|_| ErrorCode::BotIo)?;
        if sum == 0 || sum > 64 || sequence >= sum || payload.len() > 1024 * 1024 {
            return Err(ErrorCode::ResourceExhausted);
        }
        if sum == 1 {
            return Ok(Some(payload.clone()));
        }
        let id = frame
            .header("message_id")
            .filter(|id| model::valid_id(id))
            .ok_or(ErrorCode::BotIo)?;
        let now = Instant::now();
        self.0.retain(|_, parts| parts.expires > now);
        if self.0.len() >= 32 && !self.0.contains_key(id) {
            return Err(ErrorCode::ResourceExhausted);
        }
        let parts = self.0.entry(id.to_owned()).or_insert_with(|| Parts {
            expires: now + Duration::from_secs(5),
            chunks: vec![None; sum],
            bytes: 0,
        });
        if parts.chunks.len() != sum {
            return Err(ErrorCode::BotIo);
        }
        if let Some(existing) = &parts.chunks[sequence] {
            if existing != payload {
                return Err(ErrorCode::BotIo);
            }
        } else {
            parts.bytes += payload.len();
            if parts.bytes > 1024 * 1024 {
                self.0.remove(id);
                return Err(ErrorCode::ResourceExhausted);
            }
            parts.chunks[sequence] = Some(payload.clone());
        }
        if parts.chunks.iter().any(Option::is_none) {
            return Ok(None);
        }
        let parts = self.0.remove(id).ok_or(ErrorCode::BotIo)?;
        let mut output = Vec::with_capacity(parts.bytes);
        for part in parts.chunks.into_iter().flatten() {
            output.extend(part);
        }
        Ok(Some(output))
    }
}

#[cfg(test)]
mod tests;
