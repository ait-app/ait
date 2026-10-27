use std::borrow::Cow;
use std::collections::{BTreeMap, VecDeque};
use std::io::Read;
use std::path::PathBuf;

use base64::Engine;
use sha2::{Digest, Sha256};

use chrono::{TimeZone, Utc};
use serde_json::{Value, json};

use crate::local::images::ImageStore;
use crate::ports::agent_session::AgentTurnEvent;
use crate::protocol::{timeline::NativeItem, usage::AgentUsage};

const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;

/// Payload-free reasons why a native event cannot be projected.
#[derive(Debug, thiserror::Error)]
pub(super) enum Error {
    #[error("Invalid or missing OMP field: {0}.")]
    Field(&'static str),
    #[error("OMP returned a tool result without a matching tool call.")]
    UnknownTool,
    #[error("OMP exceeded the limit of 4096 pending tool calls.")]
    ToolLimit,
    #[error("OMP returned invalid token usage or cost.")]
    Usage,
    #[error("Ait could not render an OMP image.")]
    Image,
}

#[derive(Debug)]
pub(super) struct Stream {
    pub(super) events: VecDeque<AgentTurnEvent>,
    pub(super) input_ids: BTreeMap<String, Value>,
    pub(super) pending_inputs: VecDeque<Option<String>>,
    pub(super) failed: bool,
    pub(super) aborted: bool,
    pub(super) last_text: Option<String>,
    session: String,
    turn: Option<String>,
    current: Option<(String, String)>,
    counts: BTreeMap<String, usize>,
    tools: BTreeMap<String, Value>,
    observation: u64,
    usage: AgentUsage,
    images: ImageStore,
    blobs: Option<PathBuf>,
}

impl Stream {
    pub(super) fn set_context(&mut self, state: &Value) {
        self.usage.context_window_max_tokens = state["contextUsage"]["contextWindow"]
            .as_u64()
            .or_else(|| state["model"]["contextWindow"].as_u64());
        if let Some(tokens) = state["contextUsage"]["tokens"].as_u64() {
            self.usage.context_window_used_tokens = Some(tokens);
        }
    }
    pub(super) fn new(session: String, images: ImageStore) -> Self {
        Self {
            events: VecDeque::new(),
            input_ids: BTreeMap::new(),
            pending_inputs: VecDeque::new(),
            failed: false,
            aborted: false,
            last_text: None,
            session,
            turn: None,
            current: None,
            counts: BTreeMap::new(),
            tools: BTreeMap::new(),
            observation: 0,
            usage: AgentUsage::default(),
            images,
            blobs: None,
        }
    }

    /// Resolve native content-addressed images from `directory`, without changing OMP files.
    pub(super) fn with_blobs(mut self, directory: PathBuf) -> Self {
        self.blobs = Some(directory);
        self
    }

    fn image(&self, image: &Value) -> Result<String, Error> {
        let Some(hash) = image["data"]
            .as_str()
            .and_then(|data| data.strip_prefix("blob:sha256:"))
        else {
            return self.images.render(image).map_err(|_| Error::Image);
        };
        if hash.len() != 64
            || !hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(Error::Image);
        }
        let path = self.blobs.as_ref().ok_or(Error::Image)?.join(hash);
        let metadata = path.symlink_metadata().map_err(|_| Error::Image)?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_IMAGE_BYTES {
            return Err(Error::Image);
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|_| Error::Image)?
            .take(MAX_IMAGE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Image)?;
        if bytes.len() as u64 > MAX_IMAGE_BYTES || format!("{:x}", Sha256::digest(&bytes)) != hash {
            return Err(Error::Image);
        }
        let mut resolved = Cow::Borrowed(image);
        resolved.to_mut()["data"] = json!(base64::engine::general_purpose::STANDARD.encode(bytes));
        self.images.render(&resolved).map_err(|_| Error::Image)
    }

    pub(super) fn begin(&mut self, turn: String) {
        self.turn = Some(turn);
        self.failed = false;
        self.aborted = false;
        self.last_text = None;
    }

    fn anchor(&self, message: &Value) -> Result<String, Error> {
        let role =
            super::config::text(message, "role").map_err(|_| Error::Field("message.role"))?;
        let timestamp = message["timestamp"]
            .as_i64()
            .ok_or(Error::Field("message.timestamp"))?;
        Ok(format!("native:omp:{}:{role}:{timestamp}", self.session))
    }

    fn start(&mut self, message: &Value) -> Result<String, Error> {
        let anchor = self.anchor(message)?;
        let count = self.counts.entry(anchor.clone()).or_default();
        let key = format!("{anchor}:{}", *count);
        *count += 1;
        self.current = Some((anchor, key.clone()));
        Ok(key)
    }

    fn message_key(&mut self, message: &Value) -> Result<String, Error> {
        let anchor = self.anchor(message)?;
        if let Some((current, key)) = &self.current
            && current == &anchor
        {
            return Ok(key.clone());
        }
        self.start(message)
    }

    fn item(key: String, message: &Value, item: Value) -> Result<NativeItem, Error> {
        let timestamp = message["timestamp"]
            .as_i64()
            .and_then(|time| Utc.timestamp_millis_opt(time).single())
            .ok_or(Error::Field("message.timestamp"))?
            .to_rfc3339();
        Ok(NativeItem {
            key,
            turn_id: None,
            timestamp,
            item,
        })
    }

    pub(super) fn text(&self, content: &Value) -> Result<String, Error> {
        if let Some(text) = content.as_str() {
            return Ok(preview(text));
        }
        let mut parts = Vec::new();
        for block in content.as_array().ok_or(Error::Field("content"))? {
            match block["type"].as_str() {
                Some("text") => parts.push(preview(
                    block["text"].as_str().ok_or(Error::Field("content.text"))?,
                )),
                Some("image") => parts.push(self.image(block)?),
                _ => {}
            }
        }
        Ok(preview(&parts.join("\n")))
    }

    pub(super) fn message(&mut self, message: &Value) -> Result<(), Error> {
        let role = message["role"]
            .as_str()
            .ok_or(Error::Field("message.role"))?;
        if !matches!(role, "user" | "assistant" | "toolResult" | "custom") {
            return Ok(());
        }
        if role == "custom" && message["display"] == false {
            return Ok(());
        }
        let key = self.message_key(message)?;
        if role == "custom" {
            let item = self.custom_item(&key, message)?;
            self.events
                .push_back(AgentTurnEvent::Timeline(Self::item(key, message, item)?));
        } else if role == "user" {
            let mut item = json!({"type":"user_message","messageId":key,"text":self.text(&message["content"])?});
            if let Some(Some(id)) = self.pending_inputs.pop_front() {
                self.input_ids.insert(key.clone(), json!(id));
            }
            if let Some(id) = self.input_ids.get(&key) {
                item["clientMessageId"] = id.clone();
            }
            let entry = Self::item(key, message, item)?;
            self.events.push_back(AgentTurnEvent::Timeline(entry));
        } else if role == "assistant" {
            for (index, block) in message["content"]
                .as_array()
                .ok_or(Error::Field("message.content"))?
                .iter()
                .enumerate()
            {
                match block["type"].as_str() {
                    Some("text" | "thinking") => {
                        let thinking = block["type"] == "thinking";
                        let text = block[if thinking { "thinking" } else { "text" }]
                            .as_str()
                            .ok_or(Error::Field("message.content.text/thinking"))?;
                        if text.is_empty() {
                            continue;
                        }
                        if !thinking {
                            self.last_text = Some(preview(text));
                        }
                        let key = format!("{key}:{index}");
                        let item = json!({"type":if thinking{"reasoning"}else{"assistant_message"},"messageId":key,"text":preview(text)});
                        self.events
                            .push_back(AgentTurnEvent::Timeline(Self::item(key, message, item)?));
                    }
                    Some("toolCall") => {
                        let id = super::config::text(block, "id")
                            .map_err(|_| Error::Field("toolCall.id"))?
                            .to_owned();
                        if self.tools.len() >= 4096 {
                            return Err(Error::ToolLimit);
                        }
                        self.tools.insert(id, block.clone());
                    }
                    Some("image") => {
                        let key = format!("{key}:{index}");
                        let image = self.image(block)?;
                        let item = json!({"type":"assistant_message","messageId":key,"text":image});
                        self.events
                            .push_back(AgentTurnEvent::Timeline(Self::item(key, message, item)?));
                    }
                    _ => {}
                }
            }
            self.failed = message["stopReason"] == "error";
            self.aborted |= message["stopReason"] == "aborted";
            if let Some(usage) = message.get("usage") {
                self.add_usage(usage)?;
            }
        } else {
            let id = super::config::text(message, "toolCallId")
                .map_err(|_| Error::Field("message.toolCallId"))?;
            let tool = self.tools.remove(id).ok_or(Error::UnknownTool)?;
            let item = tool_item(
                &tool,
                Some(self.text(&message["content"])?),
                message["isError"] == true,
                false,
            );
            self.events.push_back(AgentTurnEvent::Timeline(Self::item(
                format!("native:omp:{}:tool:{id}", self.session),
                message,
                item,
            )?));
        }
        self.current = None;
        Ok(())
    }

    pub(super) fn consume(&mut self, event: &Value) -> Result<(), Error> {
        match event["type"].as_str() {
            Some("command_output") => self.notice(event, "text")?,
            Some("extension_ui_request") if event["method"] == "notify" => {
                self.notice(event, "message")?;
            }
            Some("message_start")
                if matches!(
                    event["message"]["role"].as_str(),
                    Some("user" | "assistant" | "toolResult" | "custom")
                ) =>
            {
                self.start(&event["message"])?;
            }
            Some("message_end") => self.message(&event["message"])?,
            Some("message_update") => {
                let update = &event["assistantMessageEvent"];
                if matches!(
                    update["type"].as_str(),
                    Some("text_delta" | "thinking_delta")
                ) {
                    let message = &event["message"];
                    let base = self.message_key(message)?;
                    let index = update["contentIndex"]
                        .as_u64()
                        .ok_or(Error::Field("assistantMessageEvent.contentIndex"))?;
                    let key = format!("{base}:{index}");
                    let item = json!({"type":if update["type"]=="thinking_delta"{"reasoning"}else{"assistant_message"},
                        "messageId":key,"text":preview(update["delta"].as_str().ok_or(Error::Field("assistantMessageEvent.delta"))?)});
                    let entry = Self::item(key, message, item)?;
                    self.progress(entry);
                }
            }
            Some("tool_execution_start" | "tool_execution_update") => {
                let id = super::config::text(event, "toolCallId")
                    .map_err(|_| Error::Field("toolCallId"))?;
                // OMP keeps reporting detached job output after its toolResult ("Backgrounded").
                // That result already finalized the timeline item; it cannot accept more progress.
                if event["type"] == "tool_execution_update" && !self.tools.contains_key(id) {
                    return Ok(());
                }
                let tool = self.tools.get(id).cloned().unwrap_or_else(
                    || json!({"id":id,"name":event["toolName"],"arguments":event["args"]}),
                );
                let output = event
                    .get("partialResult")
                    .and_then(|value| value.get("content"))
                    .map(|content| self.text(content))
                    .transpose()?;
                let item = tool_item(&tool, output, false, true);
                self.progress(NativeItem {
                    key: format!("native:omp:{}:tool:{id}", self.session),
                    turn_id: None,
                    timestamp: Utc::now().to_rfc3339(),
                    item,
                });
            }
            // AgentSession owns the settled/abort lifecycle; an agent_end may precede retry.
            _ => {}
        }
        Ok(())
    }

    fn custom_item(&self, key: &str, message: &Value) -> Result<Value, Error> {
        let text = self.text(&message["content"])?;
        let mut metadata = json!({"nativeRole":"custom","customType":message["customType"]});
        if message["customType"] != "async-result" {
            return Ok(json!({"type":"assistant_message","messageId":key,
                "text":text,"metadata":metadata}));
        }
        let jobs = message["details"]["jobs"].as_array();
        let single = jobs.filter(|jobs| jobs.len() == 1).map(|jobs| &jobs[0]);
        let job_id = single
            .and_then(|job| job["jobId"].as_str())
            .filter(|id| !id.is_empty() && id.len() <= 256);
        let call_id = job_id.map_or_else(
            || format!("{key}:background"),
            |id| format!("background:{id}"),
        );
        let status = if jobs.is_some_and(|jobs| {
            jobs.iter()
                .any(|job| matches!(job["status"].as_str(), Some("failed" | "error")))
        }) {
            "failed"
        } else if jobs.is_some_and(|jobs| {
            jobs.iter()
                .any(|job| matches!(job["status"].as_str(), Some("pending" | "running")))
        }) {
            "running"
        } else if jobs.is_some_and(|jobs| {
            !jobs.is_empty()
                && jobs.iter().all(|job| {
                    matches!(
                        job["status"].as_str(),
                        Some("cancelled" | "canceled" | "aborted")
                    )
                })
        }) {
            "canceled"
        } else {
            "completed"
        };
        let input = single
            .and_then(|job| job["label"].as_str())
            .or(job_id)
            .map_or_else(
                || {
                    jobs.filter(|jobs| !jobs.is_empty())
                        .map_or_else(String::new, |jobs| format!("{} jobs", jobs.len()))
                },
                preview,
            );
        if let Some(id) = job_id {
            metadata["jobId"] = json!(id);
        }
        Ok(json!({"type":"tool_call","callId":call_id,
            "name":if jobs.is_some_and(|jobs| jobs.len() > 1) {"Background jobs"} else {"Background job"},
            "status":status,"error":if status == "failed" {json!("Background job failed")} else {Value::Null},
            "detail":{"type":"unknown","input":input,"output":text},"metadata":metadata}))
    }

    fn notice(&mut self, event: &Value, field: &'static str) -> Result<(), Error> {
        let text = event[field].as_str().ok_or(Error::Field(field))?;
        if text.is_empty() {
            return Ok(());
        }
        let text = preview(text);
        if event["type"] != "extension_ui_request" || self.last_text.is_none() {
            self.last_text = Some(text.clone());
        }
        let key = format!(
            "native:omp:{}:notice:{}",
            self.session,
            uuid::Uuid::new_v4()
        );
        let mut item = if event["type"] == "extension_ui_request" {
            let level = match event["notifyType"].as_str() {
                Some("warning") => "warning",
                Some("error") => "error",
                _ => "info",
            };
            json!({"type":"notification","level":level,"message":text})
        } else {
            json!({"type":"assistant_message","messageId":key,"text":text})
        };
        item["metadata"] = json!({"nativeType":event["type"],
            "notificationType":event["notifyType"],"transient":true});
        self.events.push_back(AgentTurnEvent::Timeline(NativeItem {
            key,
            turn_id: self.turn.clone(),
            timestamp: Utc::now().to_rfc3339(),
            item,
        }));
        Ok(())
    }

    fn progress(&mut self, entry: NativeItem) {
        self.observation += 1;
        self.events.push_back(AgentTurnEvent::Progress {
            observation: format!(
                "{}:{}:{}",
                self.session,
                self.turn.as_deref().unwrap_or("history"),
                self.observation
            ),
            entry,
        });
    }

    fn add_usage(&mut self, usage: &Value) -> Result<(), Error> {
        let input = usage["input"].as_u64().ok_or(Error::Field("usage.input"))?;
        let output = usage["output"]
            .as_u64()
            .ok_or(Error::Field("usage.output"))?;
        let cached = usage["cacheRead"].as_u64().unwrap_or_default();
        self.usage.input_tokens = Some(
            self.usage
                .input_tokens
                .unwrap_or_default()
                .saturating_add(input),
        );
        self.usage.output_tokens = Some(
            self.usage
                .output_tokens
                .unwrap_or_default()
                .saturating_add(output),
        );
        self.usage.cached_input_tokens = Some(
            self.usage
                .cached_input_tokens
                .unwrap_or_default()
                .saturating_add(cached),
        );
        self.usage.context_window_used_tokens = usage["totalTokens"].as_u64();
        if let Some(cost) = usage["cost"]["total"].as_f64() {
            self.usage.total_cost_usd = Some(self.usage.total_cost_usd.unwrap_or_default() + cost);
        }
        if !self.usage.is_valid() {
            return Err(Error::Usage);
        }
        self.events
            .push_back(AgentTurnEvent::Usage(self.usage.clone()));
        Ok(())
    }
}

fn tool_item(tool: &Value, output: Option<String>, failed: bool, running: bool) -> Value {
    let name = tool["name"].as_str().unwrap_or("tool");
    let input = &tool["arguments"];
    let mapped = match name {
        "bash" => "Bash",
        "read" => "Read",
        "edit" => "Edit",
        "write" => "Write",
        "grep" => "Grep",
        "find" => "Glob",
        "task" => "Task",
        _ => name,
    };
    let mut parameters = input.clone();
    if let Some(path) = input.get("path") {
        parameters["file_path"] = path.clone();
    }
    if let Some(old) = input.get("oldText") {
        parameters["old_string"] = old.clone();
    }
    if let Some(new) = input.get("newText") {
        parameters["new_string"] = new.clone();
    }
    let encoded = input.to_string();
    let mut detail = if encoded.len() > 256 * 1024 {
        json!({"type":"unknown","input":preview(&encoded),"output":null})
    } else {
        crate::local::tool_detail::claude(mapped, &parameters)
    };
    if let Some(output) = output {
        crate::local::tool_detail::claude_result(&mut detail, &json!(output));
    }
    json!({"type":"tool_call","callId":tool["id"],"name":name,"detail":detail,
        "status":if running{"running"}else if failed{"failed"}else{"completed"},
        "error":if failed{json!("Native tool failed")}else{Value::Null}})
}

pub(super) fn preview(text: &str) -> String {
    let mut end = text.len().min(128 * 1024);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    if end == text.len() {
        text.to_owned()
    } else {
        format!("{}\n[Truncated; full output remains in OMP.]", &text[..end])
    }
}

#[cfg(test)]
mod tests;
