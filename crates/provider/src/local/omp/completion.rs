//! Negotiated native completion. Older official runtimes keep catalog fallback without probes.

use serde_json::{Value, json};

use super::transport::Transport;
use crate::ports::agent_session::AgentSessionError;
use crate::protocol::commands::{CompletionInput, ValidatedCompletion};

/// Query only a runtime that declared this read-only capability, preserving native edits.
/// Returns unsupported without I/O for older runtimes; malformed native results remain errors.
pub(super) async fn complete(
    transport: &mut Transport,
    input: &CompletionInput,
    native: &[Value],
    vibe: bool,
) -> Result<ValidatedCompletion, AgentSessionError> {
    input.validate()?;
    if !transport.command_completion() {
        return Ok(ValidatedCompletion::default());
    }
    let value = transport.request("complete", json!(input)).await?;
    let result = ValidatedCompletion::parse(value, input)?;
    Ok(result.retain(|item| {
        if !input.text.starts_with('/') || !input.text.is_ascii() {
            return true;
        }
        let edited = format!(
            "{}{}{}",
            &input.text[..item.replace_range.start],
            item.insert_text,
            &input.text[item.replace_range.end..]
        );
        super::commands::blocked(&edited, native, vibe).is_none()
    }))
}
