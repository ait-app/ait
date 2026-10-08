//! Bounded ACP transport and timeline projection shared by native adapters.

pub(super) mod permissions;
pub(super) mod streaming;
pub(super) mod transport;

use crate::ports::agent_session::AgentSessionError;
use serde_json::Value;

/// Read a nonempty, bounded identifier from `value[key]`; fail on malformed fields.
pub(super) fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, AgentSessionError> {
    value[key]
        .as_str()
        .filter(|text| {
            !text.is_empty() && text.len() <= 1024 && !text.chars().any(char::is_control)
        })
        .ok_or(AgentSessionError::Failed)
}

#[cfg(test)]
mod tests;
