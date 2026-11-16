//! Native content/status task lists projected into the shared TODO snapshot contract.

use provider_core::ports::agent_session::AgentSessionError;
use serde_json::{Value, json};

const MAX_ITEMS: usize = 1024;
const MAX_TEXT_BYTES: usize = 8192;
const MAX_SNAPSHOT_BYTES: usize = 128 * 1024;

/// Project complete native `entries` into a canonical TODO snapshot.
///
/// Accepts ACP v1 plan entries and DSH `todo/write` todos. The list replaces the previous
/// snapshot, including when empty; priorities and extension metadata do not affect completion.
///
/// # Errors
///
/// Returns [`AgentSessionError::Failed`] for malformed entries, unknown ACP v1 statuses,
/// or a snapshot exceeding the item, per-entry text or total serialized byte budget.
pub fn snapshot(entries: &Value) -> Result<Value, AgentSessionError> {
    let entries = entries
        .as_array()
        .filter(|entries| entries.len() <= MAX_ITEMS)
        .ok_or(AgentSessionError::Failed)?;
    let mut items = Vec::with_capacity(entries.len());
    let mut text_bytes = 0_usize;
    for entry in entries {
        let text = entry["content"]
            .as_str()
            .filter(|text| text.len() <= MAX_TEXT_BYTES)
            .ok_or(AgentSessionError::Failed)?;
        text_bytes += text.len();
        if text_bytes > MAX_SNAPSHOT_BYTES {
            return Err(AgentSessionError::Failed);
        }
        let Some(status @ ("pending" | "in_progress" | "completed")) = entry["status"].as_str()
        else {
            return Err(AgentSessionError::Failed);
        };
        items.push(json!({"text":text,"status":status,"completed":status=="completed"}));
    }
    let item = json!({"type":"todo","items":items});
    if serde_json::to_vec(&item)
        .map_err(|_| AgentSessionError::Failed)?
        .len()
        > MAX_SNAPSHOT_BYTES
    {
        return Err(AgentSessionError::Failed);
    }
    Ok(item)
}

/// Project a compatible successful native task tool's complete `input` into TODO state.
///
/// Recognizes `TodoWrite`, `todo_write` and `update_plan` schemas without a provider ID.
/// Returns `None` for unknown tools, invalid schemas or oversized content. Callers must
/// confirm native tool success before publishing this candidate snapshot.
#[must_use]
pub fn tool_input(name: &str, input: &Value) -> Option<Value> {
    let parsed;
    let input = if let Some(text) = input.as_str() {
        if text.len() > MAX_SNAPSHOT_BYTES {
            return None;
        }
        parsed = serde_json::from_str(text).ok()?;
        &parsed
    } else {
        input
    };
    let plan = name.trim().eq_ignore_ascii_case("update_plan");
    if !plan
        && !name.trim().eq_ignore_ascii_case("TodoWrite")
        && !name.trim().eq_ignore_ascii_case("todo_write")
    {
        return None;
    }
    let source = input[if plan { "plan" } else { "todos" }]
        .as_array()
        .filter(|items| items.len() <= MAX_ITEMS)?;
    let mut entries = Vec::with_capacity(source.len());
    let mut text_bytes = 0_usize;
    for item in source {
        let text = item[if plan { "step" } else { "content" }]
            .as_str()
            .filter(|text| text.len() <= MAX_TEXT_BYTES)?;
        text_bytes += text.len();
        if text_bytes > MAX_SNAPSHOT_BYTES {
            return None;
        }
        let status = match item["status"].as_str() {
            Some("inProgress") if plan => "in_progress",
            Some(status @ ("pending" | "in_progress" | "completed")) => status,
            _ if plan => "pending",
            _ => return None,
        };
        entries.push(json!({"content":text,"status":status}));
    }
    let mut projected = snapshot(&Value::Array(entries)).ok()?;
    for (item, original) in projected["items"].as_array_mut()?.iter_mut().zip(source) {
        for field in ["id", "activeForm"] {
            if let Some(text) = original[field]
                .as_str()
                .filter(|text| text.len() <= MAX_TEXT_BYTES)
            {
                item[field] = json!(text);
            }
        }
    }
    (serde_json::to_vec(&projected).ok()?.len() <= MAX_SNAPSHOT_BYTES).then_some(projected)
}

#[cfg(test)]
mod tests;
