//! Limits count newly generated native items and usage, excluding immutable prepared history.
use super::{OpenCodeExecutionLimits, Version, failure};
use crate::local::opencode::types::Snapshot;
use crate::local::opencode::types::{Fault, ProtocolError};
use serde_json::Value;
use std::collections::HashSet;

pub(super) fn validate(
    version: Version,
    raw: &[Value],
    prepared: &Snapshot,
    limits: OpenCodeExecutionLimits,
) -> Result<(), ProtocolError> {
    let known = prepared
        .messages
        .iter()
        .map(|message| message.id.as_str())
        .collect::<HashSet<_>>();
    let mut steps = 0_u64;
    let mut tokens = 0_u64;
    let mut bytes = 0_usize;
    for message in raw {
        let info = if version == Version::V1 {
            &message["info"]
        } else {
            message
        };
        let Some(id) = info.get("id").and_then(Value::as_str) else {
            continue;
        };
        if known.contains(id) {
            continue;
        }
        let parts = if version == Version::V1 {
            message.get("parts")
        } else {
            message.get("content")
        };
        if let Some(parts) = parts.and_then(Value::as_array) {
            steps = steps.saturating_add(parts.len() as u64);
            bytes = bytes.saturating_add(
                parts
                    .iter()
                    .filter_map(|part| part.get("text").and_then(Value::as_str))
                    .map(str::len)
                    .sum::<usize>(),
            );
        }
        if let Some(usage) = info.get("tokens") {
            tokens = tokens.saturating_add(
                [
                    "/input",
                    "/output",
                    "/reasoning",
                    "/cache/read",
                    "/cache/write",
                ]
                .iter()
                .filter_map(|path| usage.pointer(path).and_then(Value::as_u64))
                .fold(0_u64, u64::saturating_add),
            );
        }
    }
    if steps > limits.max_steps || tokens > limits.max_tokens || bytes > limits.max_output_bytes {
        return Err(failure(
            Fault::RunLimitExceeded,
            "OpenCode native execution exceeded its host budget",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
