//! Native ACP identities drive both streaming and authoritative history replay.
use std::{
    borrow::Cow,
    collections::{BTreeMap, VecDeque},
};

use serde_json::{Value, json};

use crate::{
    local::images::ImageStore,
    ports::agent_session::{AgentSessionError, AgentTurnEvent},
    protocol::{timeline::NativeItem, usage::AgentUsage},
};

// JSON can expand each control byte to six bytes; leave room for entry metadata.
const TEXT_BYTES: usize = 96 * 1024;
const PREVIEW_BYTES: usize = 32 * 1024;

#[derive(Debug)]
struct Text {
    source: String,
    entry: NativeItem,
    buffer: String,
    segment: usize,
}

/// Bounded native projections, sharing identities across live updates and history replay.
#[derive(Debug, Default)]
pub(super) struct Stream {
    pub(super) events: VecDeque<AgentTurnEvent>,
    pub(super) last_message: Option<String>,
    pub(super) clients: BTreeMap<String, String>,
    pub(super) users: Vec<String>,
    current: Option<Text>,
    last_source: Option<String>,
    segments: BTreeMap<String, usize>,
    tools: BTreeMap<String, Value>,
    turn: Option<String>,
    foreground_turn: Option<String>,
    observation: u64,
    images: ImageStore,
}

impl Stream {
    /// Use the supplied private image store for native embedded image content.
    pub(super) fn new(images: ImageStore) -> Self {
        Self {
            images,
            ..Self::default()
        }
    }

    /// Associate live ACP updates with the accepted input while native user IDs are unavailable.
    pub(super) fn for_turn(images: ImageStore, turn: String) -> Self {
        Self {
            foreground_turn: Some(turn),
            ..Self::new(images)
        }
    }

    /// Project submitted input before its reply; authoritative replay replaces these display rows.
    /// Image materialization and invalid content errors are returned before native submission.
    pub(super) fn submitted(
        images: ImageStore,
        turn: &str,
        blocks: &[Value],
    ) -> Result<Vec<NativeItem>, AgentSessionError> {
        let id = format!("submitted:{turn}");
        let mut stream = Self::for_turn(images, turn.to_owned());
        stream.clients.insert(id.clone(), turn.to_owned());
        for block in blocks {
            stream.chunk("user_message", &id, block)?;
        }
        stream.flush();
        Ok(stream
            .events
            .into_iter()
            .filter_map(|event| {
                if let AgentTurnEvent::Timeline(entry) = event {
                    Some(entry)
                } else {
                    None
                }
            })
            .collect())
    }

    /// Consume one ACP update; malformed supported content and resource excesses fail.
    pub(super) fn update(&mut self, update: &Value) -> Result<(), AgentSessionError> {
        match update["sessionUpdate"].as_str() {
            Some("user_message_chunk") => self.chunk(
                "user_message",
                super::config::text(update, "messageId")?,
                &update["content"],
            ),
            Some("agent_message_chunk") => self.chunk(
                "assistant_message",
                super::config::text(update, "messageId")?,
                &update["content"],
            ),
            Some("agent_thought_chunk") => self.chunk(
                "reasoning",
                super::config::text(update, "messageId")?,
                &update["content"],
            ),
            Some("tool_call" | "tool_call_update") => self.tool(update),
            Some("usage_update") => {
                let usage = AgentUsage {
                    context_window_used_tokens: update["used"].as_u64(),
                    context_window_max_tokens: update["size"].as_u64(),
                    total_cost_usd: if update["cost"]["currency"] == "USD" {
                        update["cost"]["amount"].as_f64()
                    } else {
                        None
                    },
                    ..AgentUsage::default()
                };
                if !usage.is_valid() {
                    return Err(AgentSessionError::Failed);
                }
                self.events.push_back(AgentTurnEvent::Usage(usage));
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn chunk(&mut self, kind: &str, id: &str, content: &Value) -> Result<(), AgentSessionError> {
        let delta = match content["type"].as_str() {
            Some("text") => {
                Cow::Borrowed(content["text"].as_str().ok_or(AgentSessionError::Failed)?)
            }
            Some("image") => Cow::Owned(image(&self.images, content)?),
            // Resource links are visible without reading user files on the client's behalf.
            Some("resource_link") => Cow::Owned(format!(
                "[{}]({})",
                content["name"].as_str().unwrap_or("Resource"),
                content["uri"].as_str().ok_or(AgentSessionError::Failed)?
            )),
            _ => return Err(AgentSessionError::Failed),
        };
        if delta.is_empty() {
            return Ok(());
        }
        if self
            .current
            .as_ref()
            .is_some_and(|current| current.source != id || current.entry.item["type"] != kind)
        {
            self.flush();
        }
        if self.current.is_none() {
            if kind == "user_message" {
                self.turn = Some(id.to_owned());
                if self.users.last().is_none_or(|previous| previous != id) {
                    self.users.push(id.to_owned());
                    self.last_message = None;
                    self.last_source = None;
                }
            }
            let source = format!("{id}:{kind}");
            let segment = *self.segments.entry(source).or_default();
            let mut item = json!({"type":kind,"text":"","messageId":id});
            if kind == "user_message" {
                item["clientMessageId"] = json!(self.clients.get(id).map_or(id, String::as_str));
            }
            self.current = Some(Text {
                source: id.to_owned(),
                segment,
                entry: self.entry(&format!("text:{id}:{kind}:{segment}"), item),
                buffer: String::new(),
            });
        }
        // Deterministic UTF-8 chunks keep individual display entries under the timeline budget.
        let mut remaining = delta.as_ref();
        while !remaining.is_empty() {
            let current = self.current.as_mut().ok_or(AgentSessionError::Failed)?;
            let available = TEXT_BYTES - current.buffer.len();
            let mut count = available.min(remaining.len());
            while !remaining.is_char_boundary(count) {
                count -= 1;
            }
            if count == 0 {
                self.split_text()?;
                continue;
            }
            current.buffer.push_str(&remaining[..count]);
            let mut progress = current.entry.clone();
            progress.item["text"] = json!(&remaining[..count]);
            self.observation += 1;
            self.events.push_back(AgentTurnEvent::Progress {
                observation: format!("{}:{}", progress.key, self.observation),
                entry: progress,
            });
            remaining = &remaining[count..];
        }
        Ok(())
    }

    fn split_text(&mut self) -> Result<(), AgentSessionError> {
        let current = self.current.as_ref().ok_or(AgentSessionError::Failed)?;
        let id = current.source.clone();
        let kind = current.entry.item["type"]
            .as_str()
            .ok_or(AgentSessionError::Failed)?
            .to_owned();
        self.flush();
        let segment = *self
            .segments
            .get(&format!("{id}:{kind}"))
            .ok_or(AgentSessionError::Failed)?;
        let mut item = json!({"type":kind,"text":"","messageId":id});
        if kind == "user_message" {
            item["clientMessageId"] =
                json!(self.clients.get(&id).map_or(id.as_str(), String::as_str));
        }
        self.current = Some(Text {
            source: id.clone(),
            segment,
            entry: self.entry(&format!("text:{id}:{kind}:{segment}"), item),
            buffer: String::new(),
        });
        Ok(())
    }

    fn tool(&mut self, update: &Value) -> Result<(), AgentSessionError> {
        self.flush();
        let id = super::config::text(update, "toolCallId")?;
        if self.tools.len() >= 128 && !self.tools.contains_key(id) {
            return Err(AgentSessionError::Failed);
        }
        let snapshot = self
            .tools
            .entry(id.to_owned())
            .or_insert_with(|| json!({"title":id,"status":"pending"}));
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
                snapshot[key] = if key == "content"
                    && matches!(
                        snapshot["kind"].as_str(),
                        Some("execute" | "read" | "search" | "fetch")
                    )
                    && let Some(text) = super::tool::content_text(value)
                {
                    preview(&json!(text))
                } else {
                    preview(
                        if key == "rawOutput"
                            && matches!(
                                snapshot["kind"].as_str(),
                                Some("execute" | "read" | "search" | "fetch")
                            )
                        {
                            super::tool::raw_output(value)
                        } else {
                            value
                        },
                    )
                };
            }
        }
        let status = match snapshot["status"].as_str() {
            Some("pending" | "in_progress") => "running",
            Some("completed") => "completed",
            Some("failed") => "failed",
            _ => return Err(AgentSessionError::Failed),
        };
        let item = json!({"type":"tool_call","callId":id,"name":snapshot["title"],"status":status,
            "detail":super::tool::detail(snapshot),
            "metadata":{"kind":snapshot["kind"],"title":snapshot["title"],"locations":snapshot["locations"]},
            "error":if status == "failed" {
                json!(super::tool::content_text(&snapshot["content"])
                    .or_else(|| snapshot["rawOutput"]["error"].as_str().map(str::to_owned))
                    .unwrap_or_else(|| "OpenCode tool failed".into()))
            } else { Value::Null }});
        let entry = self.entry(&format!("tool:{id}"), item);
        if status == "running" {
            self.observation += 1;
            self.events.push_back(AgentTurnEvent::Progress {
                observation: format!("{}:{}", entry.key, self.observation),
                entry,
            });
        } else {
            self.tools.remove(id);
            self.observation += 1;
            self.events.push_back(AgentTurnEvent::Progress {
                observation: format!("{}:{}", entry.key, self.observation),
                entry: entry.clone(),
            });
            self.events.push_back(AgentTurnEvent::Timeline(entry));
        }
        Ok(())
    }

    /// Complete the current deterministic text segment without changing its native identity.
    pub(super) fn flush(&mut self) {
        if let Some(mut current) = self.current.take() {
            if current.buffer.is_empty() {
                return;
            }
            self.segments.insert(
                format!(
                    "{}:{}",
                    current.source,
                    current.entry.item["type"].as_str().unwrap_or_default()
                ),
                current.segment + 1,
            );
            if current.entry.item["type"] == "assistant_message" {
                if self.last_source.as_deref() != Some(&current.source) {
                    self.last_source = Some(current.source.clone());
                    self.last_message = Some(String::new());
                }
                if let Some(message) = &mut self.last_message {
                    let mut length = (8 * 1024 * 1024 - message.len()).min(current.buffer.len());
                    while !current.buffer.is_char_boundary(length) {
                        length -= 1;
                    }
                    message.push_str(&current.buffer[..length]);
                }
            }
            current.entry.item["text"] = json!(current.buffer);
            self.events
                .push_back(AgentTurnEvent::Timeline(current.entry));
        }
    }

    fn entry(&self, key: &str, item: Value) -> NativeItem {
        NativeItem {
            key: format!("native:opencode:acp-v1:{key}"),
            turn_id: self
                .foreground_turn
                .as_ref()
                .or(self.turn.as_ref())
                .cloned(),
            // ACP omits message timestamps. The host retains the first observation timestamp
            // when this native key is replayed; session/list supplies native activity time.
            timestamp: chrono::Utc::now().to_rfc3339(),
            item,
        }
    }
}

fn image(images: &ImageStore, content: &Value) -> Result<String, AgentSessionError> {
    if content.get("data").is_none()
        && let Some(uri) = content["uri"].as_str()
    {
        let uri = reqwest::Url::parse(uri).map_err(|_| AgentSessionError::Failed)?;
        if uri.scheme() == "file" {
            let path = uri.to_file_path().map_err(|()| AgentSessionError::Failed)?;
            return images.render(&json!(path.to_string_lossy()));
        }
        return images.render(&json!({"url":uri.as_str()}));
    }
    images.render(content)
}

#[cfg(test)]
mod tests;

fn preview(value: &Value) -> Value {
    let text = value
        .as_str()
        .map_or_else(|| Cow::Owned(value.to_string()), Cow::Borrowed);
    if text.len() <= PREVIEW_BYTES {
        return value.clone();
    }
    let mut end = PREVIEW_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    json!(format!(
        "{}\n[Output truncated; full output remains in the native transcript.]",
        &text[..end]
    ))
}
