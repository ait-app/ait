//! Negotiated native completion. Older official runtimes keep catalog fallback without probes.

use serde_json::json;

use super::transport::Transport;
use crate::ports::agent_session::AgentSessionError;
use crate::protocol::commands::{CompletionInput, CompletionResult};

/// Query only a runtime that declared this read-only capability, preserving native edits.
/// Returns unsupported without I/O for older runtimes; malformed native results remain errors.
pub(super) async fn complete(
    transport: &mut Transport,
    input: &CompletionInput,
) -> Result<CompletionResult, AgentSessionError> {
    input.validate()?;
    if !transport.command_completion() {
        return Ok(CompletionResult::default());
    }
    let value = transport.request("complete", json!(input)).await?;
    let result: CompletionResult =
        serde_json::from_value(value).map_err(|_| AgentSessionError::Failed)?;
    result.validate(input)?;
    Ok(result)
}
