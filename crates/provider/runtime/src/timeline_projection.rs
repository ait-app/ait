//! Read-only Paseo display projection over immutable source events.

use std::collections::HashMap;

use domain::input_envelope;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::storage::timeline::Row;

pub(crate) mod page;

/// One inclusive interval of canonical source positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Range {
    /// First covered position.
    start_seq: u64,
    /// Last covered position.
    end_seq: u64,
}

/// Display transformations applied to a source group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Collapse {
    /// Successive updates of one tool invocation.
    ToolLifecycle,
    /// Successive plugin records with the same identity.
    Identity,
    /// Adjacent assistant text fragments.
    AssistantMerge,
    /// Adjacent reasoning fragments.
    ReasoningMerge,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// Complete display item and its exact canonical source coverage.
pub(crate) struct Entry {
    provider: String,
    /// Final projected native item.
    #[serde(deserialize_with = "deserialize_item")]
    pub(crate) item: Value,
    timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    turn_id: Option<String>,
    /// Earliest canonical source position.
    pub(crate) seq_start: u64,
    /// Latest canonical source position.
    pub(crate) seq_end: u64,
    /// Exact source coverage, including noncontiguous lifecycle updates.
    source_seq_ranges: Vec<Range>,
    collapsed: Vec<Collapse>,
}

impl From<&Row> for Entry {
    fn from(row: &Row) -> Self {
        Self {
            provider: row.provider.clone(),
            item: normalize_item(row.entry.item.clone()),
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

fn deserialize_item<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Value, D::Error> {
    Value::deserialize(deserializer).map(normalize_item)
}

/// Display form of an Ait-generated `user_message`: the text a person reads as the message, plus
/// an `origin` naming the Ait component that wrote it.
///
/// # Returns
///
/// `None` for every other item, which displays unchanged.
pub(crate) fn ait_input(item: &Value) -> Option<Value> {
    if item["type"] != "user_message" {
        return None;
    }
    let envelope = input_envelope::parse(item["text"].as_str()?)?;
    let mut projected: serde_json::Map<String, Value> = item
        .as_object()?
        .iter()
        .filter(|(key, _)| *key != "text")
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    projected.insert("text".to_owned(), Value::String(envelope.text.into_owned()));
    projected.insert(
        "origin".to_owned(),
        serde_json::to_value(envelope.origin).ok()?,
    );
    Some(Value::Object(projected))
}

fn normalize_item(mut item: Value) -> Value {
    if let Some(projected) = ait_input(&item) {
        return projected;
    }
    if item["type"] != "tool_call" {
        return item;
    }
    if matches!(
        item["detail"]["type"].as_str(),
        Some("read" | "edit" | "write")
    ) && item["detail"]["filePath"]
        .as_str()
        .is_some_and(|path| path.starts_with("proc://"))
    {
        let input = std::mem::take(&mut item["detail"]);
        item["detail"] = json!({"type":"unknown","input":input,"output":null});
        return item;
    }
    let fields: &[&str] = match item["detail"]["type"].as_str() {
        Some("write") => &["content"],
        Some("shell") => &["output", "cwd"],
        _ => return item,
    };
    if let Some(detail) = item["detail"].as_object_mut() {
        for field in fields {
            if detail.get(*field).is_some_and(Value::is_null) {
                detail.remove(*field);
            }
        }
    }
    item
}

/// Merge one adjacent text source row into its existing display group.
/// Mutates `previous` only when `incoming` has compatible type, turn and message identity.
/// Returns whether the fragment was merged; neither argument is consumed.
pub(crate) fn merge_adjacent(previous: &mut Entry, incoming: &Entry) -> bool {
    let kind = incoming.item["type"].as_str().unwrap_or_default();
    let collapse = match kind {
        "assistant_message" => Collapse::AssistantMerge,
        "reasoning" => Collapse::ReasoningMerge,
        _ => return false,
    };
    if previous.item["type"] != kind
        || previous.seq_end.checked_add(1) != Some(incoming.seq_start)
        || previous.turn_id != incoming.turn_id
        || (kind == "assistant_message"
            && incoming.item.get("messageId").is_some()
            && previous.item.get("messageId") != incoming.item.get("messageId"))
    {
        return false;
    }
    let message_id = previous.item.get("messageId").cloned();
    let mut text = match &mut previous.item["text"] {
        Value::String(text) => std::mem::take(text),
        _ => String::new(),
    };
    text.push_str(incoming.item["text"].as_str().unwrap_or_default());
    previous.item = json!({"type":kind,"text":text});
    if kind == "assistant_message"
        && message_id
            .as_ref()
            .and_then(Value::as_str)
            .is_some_and(|id| !id.is_empty())
    {
        previous.item["messageId"] = message_id.unwrap_or(Value::Null);
    }
    previous.timestamp.clone_from(&incoming.timestamp);
    previous.seq_end = incoming.seq_end;
    merge_ranges(&mut previous.source_seq_ranges, &incoming.source_seq_ranges);
    for kind in &incoming.collapsed {
        append_kind(&mut previous.collapsed, *kind);
    }
    append_kind(&mut previous.collapsed, collapse);
    true
}

/// Collapse tool lifecycles, then adjacent assistant and reasoning fragments in `rows`.
pub(crate) fn project(rows: &[Row]) -> Vec<Entry> {
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
    merge_text(merge_text(entries, "assistant_message"), "reasoning")
}

/// Return visible assistant messages whose source rows occur after `after_seq`.
///
/// Provider timeline rows may contain incremental text and tool lifecycle records. The cursor is
/// applied to the raw rows before projection so a message that started in an earlier turn cannot
/// consume the new turn's fragments. Projection then reconstructs each message, and this helper
/// applies the Bot's user-visible allowlist. Distinct messages remain distinct while their own
/// fragments are merged.
pub(crate) fn assistant_messages(rows: &[Row], after_seq: u64) -> Vec<(String, String)> {
    let mut messages: Vec<(String, String)> = Vec::new();
    let mut indexes: HashMap<String, usize> = HashMap::new();
    let rows: Vec<_> = rows
        .iter()
        .filter(|row| row.seq > after_seq)
        .cloned()
        .collect();
    for entry in project(&rows)
        .into_iter()
        .filter(|entry| entry.item["type"] == "assistant_message")
    {
        let Some(text) = entry.item["text"].as_str() else {
            continue;
        };
        if text.trim().is_empty() {
            continue;
        }
        let id = entry.item["messageId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .map_or_else(|| format!("timeline:{}", entry.seq_start), str::to_owned);
        if let Some(index) = indexes.get(&id).copied() {
            messages[index].1.push_str(text);
        } else {
            indexes.insert(id.clone(), messages.len());
            messages.push((id, text.to_owned()));
        }
    }
    messages
}

/// Return the lifecycle identity of `item`, or `None` for independent/text source rows.
pub(crate) fn identity(item: &Value) -> Option<String> {
    match item["type"].as_str()? {
        "voice" => Some(format!(
            "voice:{}/{}",
            item["sessionId"].as_str()?,
            item["id"].as_str()?
        )),
        "tool_call" => item["callId"].as_str().map(str::to_owned),
        "plugin" => Some(format!(
            "{}/{}",
            item["pluginId"].as_str()?,
            item["id"].as_str()?
        )),
        _ => None,
    }
}

/// Merge a compatible lifecycle update from `incoming` into `previous`.
/// Returns false without mutation when type or tool turn identity differs.
pub(crate) fn merge_identity(previous: &mut Entry, incoming: &Entry) -> bool {
    let kind = match incoming.item["type"].as_str() {
        Some("tool_call")
            if previous.item["type"] == "tool_call" && previous.turn_id == incoming.turn_id =>
        {
            merge_tool(&mut previous.item, &incoming.item);
            previous.seq_end = previous.seq_end.max(incoming.seq_end);
            Collapse::ToolLifecycle
        }
        Some("plugin" | "voice") if previous.item["type"] == incoming.item["type"] => {
            previous.item = incoming.item.clone();
            previous.turn_id.clone_from(&incoming.turn_id);
            previous.provider.clone_from(&incoming.provider);
            previous.seq_end = incoming.seq_end;
            Collapse::Identity
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
        && previous["detail"]["type"] != "unknown"
        && !["path", "file_path", "filePath"].iter().any(|field| {
            incoming["detail"]["input"][field]
                .as_str()
                .is_some_and(|path| path.starts_with("proc://"))
        }))
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

fn merge_text(entries: Vec<Entry>, kind: &str) -> Vec<Entry> {
    let mut output: Vec<Entry> = Vec::with_capacity(entries.len());
    for entry in entries {
        if !output
            .last_mut()
            .is_some_and(|previous| entry.item["type"] == kind && merge_adjacent(previous, &entry))
        {
            output.push(entry);
        }
    }
    output
}

fn append_kind(kinds: &mut Vec<Collapse>, kind: Collapse) {
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
