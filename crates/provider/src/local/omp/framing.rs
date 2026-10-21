use base64::Engine;
use serde_json::Value;

use crate::ports::agent_session::AgentSessionError;

const MAX_LOGICAL: usize = 64 * 1024 * 1024;

#[derive(Debug)]
struct PartialFrame {
    id: String,
    count: usize,
    length: usize,
    next: usize,
    bytes: Vec<u8>,
}

#[derive(Debug, Default)]
pub(super) struct Decoder(Option<PartialFrame>);

impl Decoder {
    pub(super) fn push(&mut self, value: Value) -> Result<Option<Value>, AgentSessionError> {
        if value["type"] != "rpc_chunk" {
            if self.0.is_some() {
                return Err(AgentSessionError::Failed);
            }
            return Ok(Some(value));
        }
        let id = super::config::text(&value, "chunkId")?;
        let number = |key| {
            value[key]
                .as_u64()
                .and_then(|number| usize::try_from(number).ok())
                .ok_or(AgentSessionError::Failed)
        };
        let index = number("index")?;
        let count = number("count")?;
        let length = number("byteLength")?;
        if count == 0 || count > 65536 || length == 0 || length > MAX_LOGICAL || index >= count {
            return Err(AgentSessionError::Failed);
        }
        if self.0.is_none() {
            if index != 0 {
                return Err(AgentSessionError::Failed);
            }
            self.0 = Some(PartialFrame {
                id: id.to_owned(),
                count,
                length,
                next: 0,
                bytes: Vec::with_capacity(length),
            });
        }
        let partial = self.0.as_mut().ok_or(AgentSessionError::Failed)?;
        if partial.id != id
            || partial.count != count
            || partial.length != length
            || partial.next != index
        {
            return Err(AgentSessionError::Failed);
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(value["data"].as_str().ok_or(AgentSessionError::Failed)?)
            .map_err(|_| AgentSessionError::Failed)?;
        if bytes.is_empty() || partial.bytes.len().saturating_add(bytes.len()) > length {
            return Err(AgentSessionError::Failed);
        }
        partial.bytes.extend_from_slice(&bytes);
        partial.next += 1;
        if partial.next < count {
            return Ok(None);
        }
        let partial = self.0.take().ok_or(AgentSessionError::Failed)?;
        if partial.bytes.len() != length {
            return Err(AgentSessionError::Failed);
        }
        let value: Value =
            serde_json::from_slice(&partial.bytes).map_err(|_| AgentSessionError::Failed)?;
        if !value.is_object() || !value["type"].is_string() || value["type"] == "rpc_chunk" {
            return Err(AgentSessionError::Failed);
        }
        Ok(Some(value))
    }
}

#[cfg(test)]
mod tests;
