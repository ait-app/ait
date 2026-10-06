//! Read-only Paseo display projection over immutable source events.

use std::collections::HashMap;

use serde::Serialize;
use serde_json::{Value, json};

use crate::storage::timeline::Row;

mod page;

pub(super) use page::{fit, select};

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct Range {
    start_seq: u64,
    end_seq: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
/// Complete display item and its exact canonical source coverage.
pub(in crate::rpc) struct Entry {
    provider: String,
    pub(in crate::rpc) item: Value,
    timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    turn_id: Option<String>,
    pub(in crate::rpc) seq_start: u64,
    pub(in crate::rpc) seq_end: u64,
    source_seq_ranges: Vec<Range>,
    collapsed: Vec<&'static str>,
}

impl From<&Row> for Entry {
    fn from(row: &Row) -> Self {
        Self {
            provider: row.provider.clone(),
            item: row.entry.item.clone(),
            timestamp: row.entry.timestamp.clone(),
            turn_id: row.entry.turn_id.clone().filter(|turn| !turn.is_empty()),
            seq_start: row.seq,
            seq_end: row.seq,
            source_seq_ranges: vec![Range {
                start_seq: row.seq,
                end_seq: row.seq,
            }],
            collapsed: Vec::new(),
        }
    }
}

/// Collapse tool lifecycles, then adjacent assistant and reasoning fragments in `rows`.
pub(in crate::rpc) fn project(rows: &[Row]) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::with_capacity(rows.len());
    let mut identities = HashMap::new();
    for row in rows {
        let entry = Entry::from(row);
        if let Some(identity) = identity(&entry.item) {
            if let Some(index) = identities.get(&identity).copied() {
                let previous = &mut entries[index];
                if merge_identity(previous, &entry) {
                    continue;
                }
            }
            identities.insert(identity, entries.len());
        }
        entries.push(entry);
    }
    merge_text(
        merge_text(entries, "assistant_message", "assistant_merge"),
        "reasoning",
        "reasoning_merge",
    )
}

fn identity(item: &Value) -> Option<String> {
    match item["type"].as_str()? {
        "tool_call" => item["callId"].as_str().map(str::to_owned),
        "plugin" => Some(format!(
            "{}/{}",
            item["pluginId"].as_str()?,
            item["id"].as_str()?
        )),
        _ => None,
    }
}

fn merge_identity(previous: &mut Entry, incoming: &Entry) -> bool {
    let kind = match incoming.item["type"].as_str() {
        Some("tool_call")
            if previous.item["type"] == "tool_call" && previous.turn_id == incoming.turn_id =>
        {
            merge_tool(&mut previous.item, &incoming.item);
            previous.seq_end = previous.seq_end.max(incoming.seq_end);
            "tool_lifecycle"
        }
        Some("plugin") if previous.item["type"] == "plugin" => {
            previous.item = incoming.item.clone();
            previous.turn_id.clone_from(&incoming.turn_id);
            previous.provider.clone_from(&incoming.provider);
            previous.seq_end = incoming.seq_end;
            "identity"
        }
        _ => return false,
    };
    previous.timestamp.clone_from(&incoming.timestamp);
    merge_ranges(&mut previous.source_seq_ranges, &incoming.source_seq_ranges);
    append_kind(&mut previous.collapsed, kind);
    true
}

fn merge_tool(previous: &mut Value, incoming: &Value) {
    let known_detail = (incoming["detail"]["type"] == "unknown"
        && previous["detail"]["type"] != "unknown")
        .then(|| previous["detail"].clone());
    let mut metadata = previous["metadata"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    if let Some(incoming_metadata) = incoming["metadata"].as_object() {
        metadata.extend(incoming_metadata.clone());
    }
    if let (Some(previous), Some(incoming)) = (previous.as_object_mut(), incoming.as_object()) {
        previous.extend(incoming.clone());
    }
    if let Some(detail) = known_detail {
        previous["detail"] = detail;
    }
    if !metadata.is_empty() {
        previous["metadata"] = Value::Object(metadata);
    }
    match incoming["status"].as_str() {
        Some("completed" | "canceled") => previous["error"] = Value::Null,
        Some("failed") if incoming.get("error").is_none() => {
            if let Some(object) = previous.as_object_mut() {
                object.remove("error");
            }
        }
        _ => {}
    }
}

fn merge_text(entries: Vec<Entry>, kind: &str, collapse: &'static str) -> Vec<Entry> {
    let mut output: Vec<Entry> = Vec::with_capacity(entries.len());
    for entry in entries {
        let previous = output.last_mut().filter(|previous| {
            previous.item["type"] == kind
                && entry.item["type"] == kind
                && previous.seq_end.checked_add(1) == Some(entry.seq_start)
                && previous.turn_id == entry.turn_id
                && (kind != "assistant_message"
                    || entry.item.get("messageId").is_none()
                    || previous.item.get("messageId") == entry.item.get("messageId"))
        });
        let Some(previous) = previous else {
            output.push(entry);
            continue;
        };
        let message_id = previous.item.get("messageId").cloned();
        let mut text = match &mut previous.item["text"] {
            Value::String(text) => std::mem::take(text),
            _ => String::new(),
        };
        text.push_str(entry.item["text"].as_str().unwrap_or_default());
        previous.item = json!({"type":kind,"text":text});
        if kind == "assistant_message"
            && message_id
                .as_ref()
                .and_then(Value::as_str)
                .is_some_and(|id| !id.is_empty())
        {
            previous.item["messageId"] = message_id.unwrap_or(Value::Null);
        }
        previous.timestamp = entry.timestamp;
        previous.seq_end = entry.seq_end;
        merge_ranges(&mut previous.source_seq_ranges, &entry.source_seq_ranges);
        for kind in entry.collapsed {
            append_kind(&mut previous.collapsed, kind);
        }
        append_kind(&mut previous.collapsed, collapse);
    }
    output
}

fn append_kind(kinds: &mut Vec<&'static str>, kind: &'static str) {
    if !kinds.contains(&kind) {
        kinds.push(kind);
    }
}

fn merge_ranges(existing: &mut Vec<Range>, incoming: &[Range]) {
    for range in incoming {
        if let Some(last) = existing
            .last_mut()
            .filter(|last| range.start_seq <= last.end_seq.saturating_add(1))
        {
            last.end_seq = last.end_seq.max(range.end_seq);
        } else {
            existing.push(*range);
        }
    }
}

#[cfg(test)]
mod tests;
