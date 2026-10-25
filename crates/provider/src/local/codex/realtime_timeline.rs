//! Canonical speech items share the durable timeline, never the text-input pipeline.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use uuid::Uuid;

use super::{discovery, native_sessions};
use crate::ports::agent_session::{AgentSessionError, AgentTurnEvent};
use crate::protocol::timeline::NativeItem;

const MAX_ITEMS: usize = 8192;
const MAX_TEXT: usize = 64 * 1024;
const MAX_BYTES: usize = 16 * 1024 * 1024;

/// Bounded canonical speech state, independent of the foreground text turn and audio endpoint.
#[derive(Debug, Default)]
pub(super) struct Timeline {
    partial: BTreeMap<String, NativeItem>,
    completed: BTreeSet<String>,
    transcripts: BTreeSet<(String, String)>,
    bytes: usize,
    /// Pending ordinary source identities referenced by canonical promotions.
    pub(super) promoted: BTreeSet<(String, String)>,
    /// Ordinary source identities already projected during realtime operation.
    pub(super) ordinary: BTreeSet<(String, String)>,
}

impl Timeline {
    /// Project a canonical notification, returning an update or ignoring unknown/late events.
    /// Returns malformed identity, role or size errors without submitting any text input.
    pub(super) fn observe(
        &mut self,
        method: &str,
        params: &Value,
    ) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        match method {
            "thread/realtime/item/started" | "thread/realtime/item/completed" => {
                let native = &params["item"];
                if native["type"] == "bemItemPromoted" {
                    if method.ends_with("/completed") {
                        self.promote(native)?;
                    }
                    return Ok(None);
                }
                let Some(mut entry) = project(native, &discovery::timestamp())? else {
                    return Ok(None);
                };
                if self.completed.contains(&entry.key) {
                    return Ok(None);
                }
                let id = native_sessions::text(native, "id")?;
                if method.ends_with("/started")
                    && self.partial.get(id).is_some_and(|old| old.key != entry.key)
                    && let Some(old) = self.partial.remove(id)
                {
                    self.bytes -= old.item["text"].as_str().map_or(0, str::len);
                }
                if self.completed.len() + self.partial.len() >= MAX_ITEMS
                    && !self.partial.contains_key(id)
                {
                    return Err(AgentSessionError::Failed);
                }
                if method.ends_with("/completed") {
                    if self
                        .partial
                        .get(id)
                        .is_some_and(|partial| partial.key == entry.key)
                        && let Some(partial) = self.partial.remove(id)
                    {
                        self.bytes -= partial.item["text"].as_str().map_or(0, str::len);
                        entry.timestamp = partial.timestamp;
                    }
                    self.remember(&entry)?;
                    self.completed.insert(entry.key.clone());
                    return Ok(Some(AgentTurnEvent::Timeline(entry)));
                }
                if native["type"] != "transcriptSegment" || self.partial.contains_key(id) {
                    return Ok(None);
                }
                entry.item["status"] = json!("streaming");
                self.reserve(entry.item["text"].as_str().map_or(0, str::len))?;
                self.partial.insert(id.to_owned(), entry.clone());
                Ok(Some(progress(entry)))
            }
            "thread/realtime/item/transcript/delta" => {
                let id = native_sessions::text(params, "itemId")?;
                let delta = params["delta"].as_str().ok_or(AgentSessionError::Failed)?;
                // The canonical start supplies role/session identity. Never guess either.
                if !self.partial.contains_key(id) {
                    return Ok(None);
                }
                if delta.is_empty() {
                    return Ok(None);
                }
                self.reserve(delta.len())?;
                let entry = self.partial.get_mut(id).ok_or(AgentSessionError::Failed)?;
                let text = entry.item["text"]
                    .as_str()
                    .ok_or(AgentSessionError::Failed)?;
                if text.len() + delta.len() > MAX_TEXT {
                    return Err(AgentSessionError::Failed);
                }
                entry.item["text"] = json!(format!("{text}{delta}"));
                Ok(Some(progress(entry.clone())))
            }
            _ => Ok(None),
        }
    }

    fn reserve(&mut self, bytes: usize) -> Result<(), AgentSessionError> {
        self.bytes = self.bytes.saturating_add(bytes);
        if self.bytes > MAX_BYTES {
            return Err(AgentSessionError::Failed);
        }
        Ok(())
    }

    /// Index completed speech for exact handoff de-duplication; reject the aggregate byte limit.
    pub(super) fn remember(&mut self, entry: &NativeItem) -> Result<(), AgentSessionError> {
        if entry.item["kind"] == "transcript" {
            let role = native_sessions::text(&entry.item, "role")?;
            let text = entry.item["text"]
                .as_str()
                .ok_or(AgentSessionError::Failed)?;
            let key = (role.to_owned(), text.trim().to_owned());
            if !self.transcripts.contains(&key) {
                self.reserve(text.len())?;
                self.transcripts.insert(key);
            }
        }
        Ok(())
    }

    /// Queue a recognized presentation reference, rejecting missing identities or excess items.
    pub(super) fn promote(&mut self, native: &Value) -> Result<(), AgentSessionError> {
        if !matches!(
            native["presentation"]["type"].as_str(),
            Some("wholeItem" | "inlineMarkdown" | "inlineVisualization")
        ) {
            return Ok(());
        }
        let key = (
            native_sessions::text(native, "turnId")?.to_owned(),
            native_sessions::text(native, "itemId")?.to_owned(),
        );
        if self.promoted.len() >= MAX_ITEMS {
            return Err(AgentSessionError::Failed);
        }
        self.promoted.insert(key);
        Ok(())
    }

    /// Record an ordinary source already displayed, rejecting missing IDs or excess items.
    pub(super) fn ordinary(&mut self, turn: &str, native: &Value) -> Result<(), AgentSessionError> {
        if self.ordinary.len() >= MAX_ITEMS {
            return Err(AgentSessionError::Failed);
        }
        self.ordinary.insert((
            turn.to_owned(),
            native_sessions::text(native, "id")?.to_owned(),
        ));
        Ok(())
    }

    /// Remove only envelope excerpts already represented by canonical speech, preserving new input.
    pub(super) fn retain_handoff(&self, entry: &mut NativeItem) -> bool {
        let Some(entries) = entry.item["voiceTranscript"].as_array() else {
            return true;
        };
        let mut retained: Vec<_> = entries
            .iter()
            .filter(|part| {
                !self.contains(
                    part["role"].as_str().unwrap_or(""),
                    part["text"].as_str().unwrap_or(""),
                )
            })
            .cloned()
            .collect();
        if retained.len() == entries.len() {
            return true;
        }
        if retained.is_empty() {
            if self.contains("user", entry.item["text"].as_str().unwrap_or("")) {
                return false;
            }
            if let Some(object) = entry.item.as_object_mut() {
                object.remove("voiceTranscript");
            }
        } else {
            let input = entry.item["text"].as_str().unwrap_or("");
            if !input.trim().is_empty()
                && !self.contains("user", input)
                && !retained
                    .iter()
                    .any(|part| part["role"] == "user" && part["text"] == input)
            {
                retained.push(json!({"role":"user","text":input}));
            }
            entry.item["voiceTranscript"] = json!(retained);
        }
        true
    }

    fn contains(&self, role: &str, text: &str) -> bool {
        self.transcripts
            .contains(&(role.to_owned(), text.trim().to_owned()))
    }
}

fn progress(entry: NativeItem) -> AgentTurnEvent {
    AgentTurnEvent::Progress {
        observation: Uuid::new_v4().to_string(),
        entry,
    }
}

/// Convert one canonical speech item to a provider-neutral snapshot with a stable native key.
/// Returns malformed IDs, roles, outcomes or oversized text; future item kinds are ignored.
pub(super) fn project(
    native: &Value,
    timestamp: &str,
) -> Result<Option<NativeItem>, AgentSessionError> {
    let kind = native_sessions::text(native, "type")?;
    if !matches!(
        kind,
        "transcriptSegment" | "realtimeSessionStarted" | "realtimeSessionClosed"
    ) {
        return Ok(None);
    }
    let id = native_sessions::text(native, "id")?;
    let session = native_sessions::text(native, "realtimeSessionId")?;
    let mut item = json!({"type":"voice","id":id,"sessionId":session});
    if kind == "transcriptSegment" {
        let role = native_sessions::text(native, "role")?;
        let text = native["text"]
            .as_str()
            .filter(|text| text.len() <= MAX_TEXT)
            .ok_or(AgentSessionError::Failed)?;
        if !matches!(role, "user" | "assistant") {
            return Err(AgentSessionError::Failed);
        }
        item["kind"] = json!("transcript");
        item["role"] = json!(role);
        item["text"] = json!(text);
        item["status"] = json!("completed");
    } else {
        item["kind"] = json!("session");
        let status = if kind == "realtimeSessionStarted" {
            "started"
        } else {
            match native["outcome"].as_str() {
                Some("ended") => "ended",
                Some("failed") => "failed",
                _ => return Err(AgentSessionError::Failed),
            }
        };
        item["status"] = json!(status);
    }
    Ok(Some(NativeItem {
        key: format!("native:voice:{session}:{id}"),
        turn_id: None,
        timestamp: timestamp.to_owned(),
        item,
    }))
}

#[cfg(test)]
mod tests;
