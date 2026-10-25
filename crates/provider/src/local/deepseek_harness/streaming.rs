use std::collections::{BTreeMap, BTreeSet, VecDeque};

use chrono::Utc;
use serde_json::{Value, json};

use super::config::text;
use crate::ports::agent_session::{AgentSessionError, AgentTurnEvent};
use crate::protocol::{timeline::NativeItem, usage::AgentUsage};

const MAX_TEXT: usize = 128 * 1024;
const MAX_ITEMS: usize = 4096;

#[derive(Debug)]
struct Text {
    source: Option<String>,
    entry: NativeItem,
    buffer: String,
}

#[derive(Debug, Default)]
pub(super) struct Stream {
    pub(super) events: VecDeque<AgentTurnEvent>,
    current: Option<Text>,
    tools: BTreeMap<String, Value>,
    completed_tools: BTreeSet<String>,
    turn: String,
    sequence: usize,
    observation: usize,
    pub(super) last_message: Option<String>,
    images: crate::local::images::ImageStore,
    compactions: crate::local::compaction::Compactions,
    /// Opening identity for older journals whose compaction end omits a native ID.
    pub(super) native_compaction: Option<String>,
}

impl Stream {
    pub(super) fn new(images: crate::local::images::ImageStore) -> Self {
        Self {
            images,
            ..Self::default()
        }
    }

    pub(super) fn begin(&mut self, turn: String) {
        self.turn = turn;
        self.sequence = 0;
        self.observation = 0;
        self.last_message = None;
        self.current = None;
        self.tools.clear();
        self.completed_tools.clear();
        self.compactions = crate::local::compaction::Compactions::default();
        self.native_compaction = None;
    }

    pub(super) fn update(&mut self, update: &Value) -> Result<(), AgentSessionError> {
        match update["sessionUpdate"].as_str() {
            Some("agent_message_chunk") => self.chunk("assistant_message", update),
            Some("agent_thought_chunk") => self.chunk("reasoning", update),
            Some("tool_call" | "tool_call_update") => self.tool(update),
            Some("compaction_update") => {
                let status = match update["status"].as_str() {
                    Some("in_progress") => "loading",
                    Some("completed") => "completed",
                    Some("failed" | "cancelled") => "failed",
                    _ => return Ok(()),
                };
                self.compaction(text(update, "compactionId")?, status, None)
            }
            Some("usage_update") => {
                let usage = AgentUsage {
                    context_window_used_tokens: Some(
                        update["used"].as_u64().ok_or(AgentSessionError::Failed)?,
                    ),
                    context_window_max_tokens: Some(
                        update["size"].as_u64().ok_or(AgentSessionError::Failed)?,
                    ),
                    ..AgentUsage::default()
                };
                if !usage.is_valid() {
                    return Err(AgentSessionError::Failed);
                }
                self.events.push_back(AgentTurnEvent::Usage(usage));
                Ok(())
            }
            // Harness currently omits plans/commands/modes. Unknown extension notifications
            // have no authority over the host's lifecycle or execution policy.
            _ => Ok(()),
        }
    }

    /// Publish one bounded, deduplicated native compaction lifecycle marker.
    /// Invalid marker identities or metadata return a protocol error.
    pub(super) fn compaction(
        &mut self,
        id: &str,
        status: &'static str,
        trigger: Option<&str>,
    ) -> Result<(), AgentSessionError> {
        self.flush();
        if let Some(item) = self.compactions.marker(id, status, trigger)? {
            let entry = self.entry(
                format!("native:{}:compaction:{id}:{status}", self.turn),
                item,
            );
            self.events.push_back(AgentTurnEvent::Timeline(entry));
        }
        Ok(())
    }

    /// Stop pending loading markers when native execution ends without closing them.
    pub(super) fn finish_compactions(&mut self) {
        for item in self.compactions.fail_pending() {
            let id = item["compactionId"]
                .as_str()
                .expect("native compaction identity");
            let entry = self.entry(format!("native:{}:compaction:{id}:failed", self.turn), item);
            self.events.push_back(AgentTurnEvent::Timeline(entry));
        }
    }

    fn chunk(&mut self, kind: &str, update: &Value) -> Result<(), AgentSessionError> {
        let delta = match update["content"]["type"].as_str() {
            Some("text") => std::borrow::Cow::Borrowed(
                update["content"]["text"]
                    .as_str()
                    .ok_or(AgentSessionError::Failed)?,
            ),
            Some("image") if kind == "assistant_message" => {
                std::borrow::Cow::Owned(self.images.render(&update["content"])?)
            }
            _ => return Err(AgentSessionError::Failed),
        };
        if delta.is_empty() {
            return Ok(());
        }
        let source = update
            .get("messageId")
            .map(|_| text(update, "messageId").map(str::to_owned))
            .transpose()?;
        if self
            .current
            .as_ref()
            .is_some_and(|current| current.source != source || current.entry.item["type"] != kind)
        {
            self.flush();
        }
        if self.current.is_none() {
            self.sequence += 1;
            if self.sequence > MAX_ITEMS {
                return Err(AgentSessionError::Failed);
            }
            let key = format!("native:{}:text:{}", self.turn, self.sequence);
            let mut item = json!({"type":kind,"text":""});
            if kind == "assistant_message" {
                item["messageId"] = json!(key);
            }
            self.current = Some(Text {
                source,
                entry: self.entry(key, item),
                buffer: String::new(),
            });
        }
        let current = self.current.as_mut().ok_or(AgentSessionError::Failed)?;
        if current.buffer.len().saturating_add(delta.len()) > MAX_TEXT {
            return Err(AgentSessionError::Failed);
        }
        // Keep the accumulated text separate from the transmitted delta: progress is appended
        // by the existing timeline store, while the completed item replaces that progress.
        current.buffer.push_str(&delta);
        let mut progress = current.entry.clone();
        progress.item["text"] = json!(delta);
        self.observation += 1;
        self.events.push_back(AgentTurnEvent::Progress {
            observation: format!("{}:{}", self.turn, self.observation),
            entry: progress,
        });
        Ok(())
    }

    fn tool(&mut self, update: &Value) -> Result<(), AgentSessionError> {
        self.flush();
        let id = text(update, "toolCallId")?;
        if !self.tools.contains_key(id)
            && (self.tools.len() >= 128
                || self.tools.len() + self.completed_tools.len() >= MAX_ITEMS)
        {
            return Err(AgentSessionError::Failed);
        }
        if self.completed_tools.contains(id) {
            return Err(AgentSessionError::Failed);
        }
        let snapshot = self
            .tools
            .entry(id.to_owned())
            .or_insert_with(|| json!({"title":id,"status":"pending"}));
        let fields = snapshot.as_object_mut().ok_or(AgentSessionError::Failed)?;
        for key in [
            "title",
            "kind",
            "status",
            "content",
            "locations",
            "rawInput",
            "rawOutput",
        ] {
            if let Some(value) = update.get(key) {
                let projected = if key == "content" {
                    tool_content(&self.images, value)?
                } else {
                    preview(value)
                };
                fields.insert(key.to_owned(), projected);
            }
        }
        let status = match snapshot["status"].as_str() {
            Some("pending" | "in_progress") => "running",
            Some("completed") => "completed",
            Some("failed") => "failed",
            _ => return Err(AgentSessionError::Failed),
        };
        let item = json!({"type":"tool_call","callId":id,"name":snapshot["title"],"status":status,
            "detail":{"type":"unknown","input":snapshot["rawInput"],
                "output":snapshot.get("rawOutput").unwrap_or(&snapshot["content"])},
            "metadata":{"kind":snapshot["kind"],"title":snapshot["title"]},
            "error":if status == "failed" { json!({"message":"Harness tool failed"}) } else { Value::Null }});
        let entry = self.entry(format!("native:{}:tool:{id}", self.turn), item);
        if status == "running" {
            self.observation += 1;
            self.events.push_back(AgentTurnEvent::Progress {
                observation: format!("{}:{}", self.turn, self.observation),
                entry,
            });
        } else {
            self.events.push_back(AgentTurnEvent::Timeline(entry));
            self.tools.remove(id);
            self.completed_tools.insert(id.to_owned());
        }
        Ok(())
    }

    pub(super) fn flush(&mut self) {
        if let Some(mut current) = self.current.take() {
            current.entry.item["text"] = json!(current.buffer);
            if current.entry.item["type"] == "assistant_message" {
                self.last_message = current.entry.item["text"].as_str().map(str::to_owned);
            }
            self.events
                .push_back(AgentTurnEvent::Timeline(current.entry));
        }
    }

    fn entry(&self, key: String, item: Value) -> NativeItem {
        NativeItem {
            key,
            turn_id: Some(self.turn.clone()),
            timestamp: Utc::now().to_rfc3339(),
            item,
        }
    }
}

fn preview(value: &Value) -> Value {
    const MAX_PREVIEW: usize = 32 * 1024;
    let encoded = value.to_string();
    if encoded.len() <= MAX_PREVIEW {
        return value.clone();
    }
    let mut end = MAX_PREVIEW;
    while !encoded.is_char_boundary(end) {
        end -= 1;
    }
    json!(format!(
        "{}\n[Output truncated; full output remains in the native transcript.]",
        &encoded[..end]
    ))
}

fn tool_content(
    images: &crate::local::images::ImageStore,
    value: &Value,
) -> Result<Value, AgentSessionError> {
    let blocks = value.as_array().ok_or(AgentSessionError::Failed)?;
    let rendered = blocks.iter().map(|block| {
        if block["type"] == "content" && block["content"]["type"] == "image" {
            Ok(json!({"type":"content","content":{"type":"text","text":images.render(&block["content"])?}}))
        } else { Ok(block.clone()) }
    }).collect::<Result<Vec<_>, AgentSessionError>>()?;
    Ok(preview(&Value::Array(rendered)))
}

#[cfg(test)]
mod tests;
