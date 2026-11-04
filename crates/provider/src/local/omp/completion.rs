//! Negotiated native completion. Older official runtimes keep catalog fallback without probes.

use serde_json::json;

use super::transport::Transport;
use crate::ports::agent_session::AgentSessionError;
use crate::protocol::commands::{CompletionInput, ValidatedCompletion};

/// Query only a runtime that declared this read-only capability, preserving native edits.
/// Returns unsupported without I/O for older runtimes; malformed native results remain errors.
pub(super) async fn complete(
    transport: &mut Transport,
    input: &CompletionInput,
) -> Result<ValidatedCompletion, AgentSessionError> {
    input.validate()?;
    if !transport.command_completion() {
        return Ok(ValidatedCompletion::default());
    }
    let value = transport.request("complete", json!(input)).await?;
    ValidatedCompletion::parse(value, input)
}
