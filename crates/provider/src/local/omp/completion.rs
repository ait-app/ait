//! Negotiated native completion. Older official runtimes keep catalog fallback without probes.

use serde_json::{Value, json};

use super::transport::Transport;
use crate::ports::agent_session::AgentSessionError;
use crate::protocol::commands::{CompletionInput, CompletionItem, ValidatedCompletion};

/// Query only a runtime that declared this read-only capability, preserving native edits.
/// Returns unsupported without I/O for older runtimes; malformed native results remain errors.
pub(super) async fn complete(
    transport: &mut Transport,
    input: &CompletionInput,
    native: &[Value],
) -> Result<ValidatedCompletion, AgentSessionError> {
    input.validate()?;
    if !transport.command_completion() {
        return Ok(ValidatedCompletion::default());
    }
    let value = transport.request("complete", json!(input)).await?;
    let result = ValidatedCompletion::parse(value, input)?;
    let mut produced = String::new();
    Ok(result.retain(|item| {
        // Candidates are attributed to the source that owns the *resulting* command name, so
        // an unavailable builtin never reappears here even when the accepted edit is what
        // creates it. `blocked` and the prompt guard share one name rule; the catalog stays
        // the only owner of provenance, and unknown or absent names keep dispatch.
        edited_command(&input.text, item, &mut produced)
            .is_some_and(|edited| super::commands::blocked(edited, native).is_none())
    }))
}

/// Apply one validated native edit into `into` and keep only the command name it produces.
/// Attribution never needs argument text: the name ends at the first whitespace or `:`, which
/// is exactly where the dispatch guard splits. Offsets are UTF-16 code units, not byte indices.
fn edited_command<'a>(text: &str, item: &CompletionItem, into: &'a mut String) -> Option<&'a str> {
    let start = byte_offset(text, item.replace_range.start)?;
    let end = byte_offset(text, item.replace_range.end)?;
    let tail = &text[end..];
    let tail_len = tail
        .find(|character: char| character.is_whitespace() || character == ':')
        .unwrap_or(tail.len());
    into.clear();
    into.push_str(&text[..start]);
    into.push_str(&item.insert_text);
    into.push_str(&tail[..tail_len]);
    Some(into.as_str())
}

/// Convert a validated UTF-16 offset into a byte offset; `None` if it bisects a character.
fn byte_offset(text: &str, offset: usize) -> Option<usize> {
    let mut units = 0;
    for (byte, character) in text.char_indices() {
        if units == offset {
            return Some(byte);
        }
        units += character.len_utf16();
        if units > offset {
            return None;
        }
    }
    (units == offset).then_some(text.len())
}
