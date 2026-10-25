//! Bounded canonical history with a narrow fallback for older app-server versions.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::{Transport, discovery, native_sessions, realtime_timeline};
use crate::ports::agent_session::AgentSessionError;
use crate::protocol::timeline::NativeItem;

const MAX_ENTRIES: usize = 32_768;
const MAX_BYTES: usize = 32 * 1024 * 1024;

/// Read every bounded canonical page, returning `None` only when the RPC is unsupported.
/// Returns transport, malformed/cyclic pages, inconsistent order or projection errors.
pub(super) async fn read(
    transport: &mut Transport,
    thread: &Value,
    images: &crate::local::images::ImageStore,
) -> Result<Option<Vec<NativeItem>>, AgentSessionError> {
    let id = native_sessions::text(thread, "id")?;
    let mut cursor = None::<String>;
    let mut cursors = BTreeSet::new();
    let mut entries = Vec::new();
    let mut bytes = 0;
    let mut position = None;
    loop {
        let result = transport
            .request(
                "thread/timeline/list",
                json!({"threadId":id,"cursor":cursor,"limit":100}),
            )
            .await;
        let page = match result {
            Err(AgentSessionError::Unavailable) if cursor.is_none() => return Ok(None),
            result => result?,
        };
        let data = page["data"].as_array().ok_or(AgentSessionError::Failed)?;
        bytes += serde_json::to_vec(&page)
            .map_err(|_| AgentSessionError::Failed)?
            .len();
        if bytes > MAX_BYTES || entries.len() + data.len() > MAX_ENTRIES {
            return Err(AgentSessionError::Failed);
        }
        // Pages arrive newest first, but rows within each page are chronological.
        // Accumulate in reverse rollout order so boundaries share the same position check.
        for entry in data.iter().rev() {
            let next = entry["position"]
                .as_u64()
                .ok_or(AgentSessionError::Failed)?;
            if position.is_some_and(|position| next >= position) {
                return Err(AgentSessionError::Failed);
            }
            position = Some(next);
            entries.push(entry.clone());
        }
        cursor = match &page["nextCursor"] {
            Value::Null => break,
            Value::String(cursor) if !cursor.is_empty() => Some(cursor.clone()),
            _ => return Err(AgentSessionError::Failed),
        };
        if cursors.len() >= 512 || !cursors.insert(cursor.clone()) {
            return Err(AgentSessionError::Failed);
        }
    }
    entries.reverse();
    project(&entries, thread, images).map(Some)
}

fn project(
    entries: &[Value],
    thread: &Value,
    images: &crate::local::images::ImageStore,
) -> Result<Vec<NativeItem>, AgentSessionError> {
    let mut voice = realtime_timeline::Timeline::default();
    let mut ordinary = BTreeMap::new();
    let mut stamps = BTreeMap::new();
    let created = native_sessions::timestamp(&thread["createdAt"])?;
    // Index first: promoted items and delegation excerpts may precede their final source row.
    for entry in entries {
        match entry["type"].as_str() {
            Some("item") => {
                let turn = native_sessions::text(entry, "turnId")?;
                let id = native_sessions::text(&entry["item"], "id")?;
                ordinary.insert((turn, id), &entry["item"]);
            }
            Some("turnStarted") if !entry["startedAt"].is_null() => {
                stamps.insert(
                    native_sessions::text(entry, "turnId")?,
                    native_sessions::timestamp(&entry["startedAt"])?,
                );
            }
            Some("realtime") => {
                if let Some(item) = realtime_timeline::project(&entry["item"], &created)? {
                    voice.remember(&item)?;
                }
            }
            _ => {}
        }
    }
    let mut identities = BTreeSet::new();
    let mut output = Vec::new();
    let mut timestamp = created;
    for entry in entries {
        let projected = match entry["type"].as_str() {
            Some("turnStarted") => {
                if let Some(stamp) = entry["turnId"].as_str().and_then(|turn| stamps.get(turn)) {
                    timestamp.clone_from(stamp);
                }
                Vec::new()
            }
            Some("item") => {
                let turn = native_sessions::text(entry, "turnId")?;
                discovery::timeline_items(
                    &entry["item"],
                    turn,
                    stamps.get(turn).unwrap_or(&timestamp),
                    images,
                )?
            }
            Some("realtime") if entry["item"]["type"] == "bemItemPromoted" => {
                let native = &entry["item"];
                let turn = native_sessions::text(native, "turnId")?;
                let id = native_sessions::text(native, "itemId")?;
                // Keep one full source item, including its Markdown/visualizations, for every
                // presentation variant. The promotion is an association, not a second message.
                if let Some(item) = ordinary.get(&(turn, id)) {
                    discovery::timeline_items(
                        item,
                        turn,
                        stamps.get(turn).unwrap_or(&timestamp),
                        images,
                    )?
                } else {
                    return Err(AgentSessionError::Failed);
                }
            }
            Some("realtime") => realtime_timeline::project(&entry["item"], &timestamp)?
                .into_iter()
                .collect(),
            Some("turnCompleted") => Vec::new(),
            _ => return Err(AgentSessionError::Failed),
        };
        for mut item in projected {
            if voice.retain_handoff(&mut item) && identities.insert(item.key.clone()) {
                output.push(item);
            }
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests;
