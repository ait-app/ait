use std::borrow::Cow;
use std::collections::{BTreeMap, VecDeque};

use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::local::{compaction::Compactions, diagnostics::classify};
use crate::ports::{
    agent_session::{AgentSessionError, AgentTurnEvent},
    diagnostics::{Category, Diagnostic},
};
use crate::protocol::{timeline::NativeItem, usage::AgentUsage};

// Storage caps serialized NativeItem at 768 KiB. Sixfold JSON escaping of 96 KiB
// leaves 192 KiB for metadata, matching OpenCode's conservative text allowance.
const MAX_TEXT: usize = 96 * 1024;
const MAX_ITEMS: usize = 4096;
const MAX_EVENTS: usize = 256;
const MAX_BUFFERED_EVENTS: usize = 4096;
const MAX_BUFFERED_BYTES: usize = 64 * 1024 * 1024;
const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_TOOLS: usize = 128;

#[derive(Debug)]
struct Tool {
    shell: bool,
    name: String,
    input: Value,
    input_bytes: usize,
}

#[derive(Debug)]
struct Compaction {
    id: String,
    turn: Option<String>,
}

/// Bounded per-turn display projection with session-scoped compaction identity.
#[derive(Debug, Default)]
pub(super) struct Stream {
    /// Events awaiting publication; insertion reserves room for terminal cleanup.
    pub(super) events: VecDeque<AgentTurnEvent>,
    turn: String,
    message: usize,
    observation: usize,
    bytes: usize,
    queued_tool_bytes: usize,
    queued_tools: BTreeMap<String, usize>,
    /// RPC waits cannot be drained by the manager; retain a larger, still bounded projection.
    pub(super) buffering: bool,
    backlog_capacity: usize,
    tools: BTreeMap<String, Tool>,
    compactions: Compactions,
    compaction: Option<Compaction>,
    /// Last authoritative assistant response.
    pub(super) last_text: String,
    /// Native run activity observed since admission began.
    pub(super) running: bool,
    /// A terminal native settlement or an idle handled command was observed.
    pub(super) settled: bool,
    /// Safe final native error facts, cleared when a retry succeeds.
    pub(super) failure: Option<Diagnostic>,
    pub(super) explanation: crate::local::diagnostics::failure::Failure,
}

impl Stream {
    /// Build submitted input rows, materializing images before native admission.
    pub(super) fn submitted(
        images: &crate::local::images::ImageStore,
        turn: &str,
        prompt: &crate::protocol::prompt::AgentPrompt,
        text: &str,
    ) -> Result<Vec<AgentTurnEvent>, AgentSessionError> {
        let sources = prompt
            .images
            .iter()
            .map(|image| {
                images
                    .source(&json!({"data": &image.data, "mimeType": &image.mime_type}))
                    .map(Cow::into_owned)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self::user_entries(
            turn,
            prompt.client_message_id.as_deref(),
            text,
            &sources,
        ))
    }

    /// Project user text and image references, attaching images only to the first chunk.
    /// Returns timeline rows with stable native keys and the first row's client identity.
    pub(super) fn user_entries(
        turn: &str,
        client_message_id: Option<&str>,
        text: &str,
        sources: &[String],
    ) -> Vec<AgentTurnEvent> {
        let mut remaining = text;
        let mut index = 0;
        let mut events = Vec::new();
        while !remaining.is_empty() || (index == 0 && !sources.is_empty()) {
            let end = remaining.floor_char_boundary(MAX_TEXT.min(remaining.len()));
            let suffix = format!("user:{index}");
            let message_id = if index == 0 {
                format!("native:pi:{turn}:user")
            } else {
                format!("native:pi:{turn}:{suffix}")
            };
            let mut entry = NativeItem {
                key: format!("native:pi:{turn}:{suffix}"),
                turn_id: Some(turn.to_owned()),
                timestamp: Utc::now().to_rfc3339(),
                item: json!({
                    "type":"user_message",
                    "messageId":message_id,
                    "text":&remaining[..end]
                }),
            };
            // Only the first row reconciles the local submission. Continuations must
            // have independent identities or the client collapses them into that row.
            if index == 0 {
                if let Some(id) = client_message_id {
                    entry.item["clientMessageId"] = json!(id);
                }
                if !sources.is_empty() {
                    entry.item["imageSources"] = json!(sources);
                }
            }
            events.push(AgentTurnEvent::Timeline(entry));
            remaining = &remaining[end..];
            index += 1;
        }
        events
    }

    /// Project a completed historical assistant message without live-turn display budgets.
    pub(super) fn historical_message(
        &mut self,
        message: &Value,
    ) -> Result<Vec<(usize, NativeItem)>, AgentSessionError> {
        self.message += 1;
        if self.message > MAX_ITEMS {
            return Err(AgentSessionError::Failed);
        }
        let blocks = message["content"]
            .as_array()
            .filter(|blocks| blocks.len() <= MAX_ITEMS)
            .ok_or(AgentSessionError::Failed)?;
        let mut entries = Vec::with_capacity(blocks.len());
        for (block_index, block) in blocks.iter().enumerate() {
            let (kind, field) = match block["type"].as_str() {
                Some("text") => ("assistant_message", "text"),
                Some("thinking") => ("reasoning", "thinking"),
                _ => continue,
            };
            let mut remaining = block[field].as_str().ok_or(AgentSessionError::Failed)?;
            let mut chunk_index = 0;
            loop {
                let end = remaining.floor_char_boundary(MAX_TEXT.min(remaining.len()));
                let suffix = if chunk_index == 0 {
                    format!("message:{}:{block_index}", self.message)
                } else {
                    format!("message:{}:{block_index}:{chunk_index}", self.message)
                };
                let key = format!("native:pi:{}:{suffix}", self.turn);
                entries.push((
                    block_index,
                    self.entry(
                        &suffix,
                        json!({"type":kind,"messageId":key,"text":&remaining[..end]}),
                    ),
                ));
                if end == remaining.len() {
                    break;
                }
                remaining = &remaining[end..];
                chunk_index += 1;
            }
        }
        Ok(entries)
    }

    /// Reset turn projection while retaining session-level compaction state.
    pub(super) fn begin(&mut self, turn: String) {
        let compactions = std::mem::take(&mut self.compactions);
        let compaction = self.compaction.take();
        let events = std::mem::take(&mut self.events);
        let queued_tools = std::mem::take(&mut self.queued_tools);
        let queued_tool_bytes = self.queued_tool_bytes;
        *self = Self {
            events,
            turn,
            queued_tool_bytes,
            queued_tools,
            compactions,
            compaction,
            ..Self::default()
        };
    }

    /// Reopen observed extension work without publishing an earlier intermediate settlement.
    pub(super) fn resume_running(&mut self) {
        if self.settled {
            // Admission also resumes already-running work; preserve that run's result.
            self.failure = None;
            self.explanation.clear();
            self.last_text.clear();
        }
        self.events.retain(|event| {
            !matches!(
                event,
                AgentTurnEvent::Completed(_) | AgentTurnEvent::Cancelled | AgentTurnEvent::Failed
            )
        });
        self.running = true;
        self.settled = false;
    }

    fn entry(&self, suffix: &str, item: Value) -> NativeItem {
        NativeItem {
            key: format!("native:pi:{}:{suffix}", self.turn),
            turn_id: Some(self.turn.clone()),
            timestamp: Utc::now().to_rfc3339(),
            item,
        }
    }

    /// Restore normal polling budgets after an undrainable RPC wait.
    pub(super) fn end_buffering(&mut self) {
        self.buffering = false;
        self.backlog_capacity = if self.events.len() + self.tools.len() + 2 >= MAX_EVENTS
            || self.queued_tool_bytes >= MAX_BYTES
        {
            MAX_BUFFERED_EVENTS
        } else {
            MAX_EVENTS
        };
        self.bytes = 0;
    }

    /// Drain display output, restoring the ordinary queue cap once RPC backlog fits.
    pub(super) fn pop(&mut self) -> Option<AgentTurnEvent> {
        let event = self.events.pop_front();
        if let Some(AgentTurnEvent::Progress { entry, .. } | AgentTurnEvent::Timeline(entry)) =
            &event
            && let Some(bytes) = self.queued_tools.remove(&entry.key)
        {
            self.queued_tool_bytes -= bytes;
        }
        if self.events.len() + self.tools.len() + 2 < MAX_EVENTS
            && self.queued_tool_bytes + self.bytes < MAX_BYTES
        {
            self.backlog_capacity = MAX_EVENTS;
        }
        event
    }

    fn event_limit(&self) -> usize {
        if self.buffering {
            MAX_BUFFERED_EVENTS
        } else {
            self.backlog_capacity.max(MAX_EVENTS)
        }
    }

    fn display_limit(&mut self) -> AgentSessionError {
        self.failure = Some(Diagnostic::new(Category::Protocol, "display_limit"));
        AgentSessionError::Failed
    }

    fn push(&mut self, event: AgentTurnEvent) -> Result<(), AgentSessionError> {
        if self.buffering && self.coalesce(&event) {
            return Ok(());
        }
        // Every pending tool, one compaction and the final status must fit even on failure.
        if self.events.len() + self.tools.len() + 2 >= self.event_limit() {
            return Err(self.display_limit());
        }
        self.events.push_back(event);
        Ok(())
    }

    // Text progress is additive: preserve FIFO chunks for each native key. Storage's
    // timeline::progress::{completion, suffix} requires the completed text to start
    // with all published deltas concatenated in order. Only unpublished progress
    // can be replaced by the authoritative Timeline item below.
    fn coalesce(&mut self, event: &AgentTurnEvent) -> bool {
        match event {
            AgentTurnEvent::Timeline(entry) => {
                self.events.retain(|pending| {
                    !matches!(pending,
                    AgentTurnEvent::Progress { entry: prior, .. } if prior.key == entry.key)
                });
            }
            AgentTurnEvent::Progress { entry, .. } => {
                // Search newest first: streaming deltas and tool snapshots are usually adjacent.
                if let Some(AgentTurnEvent::Progress { entry: prior, .. }) =
                    self.events.iter_mut().rev().find(|pending| {
                        matches!(pending,
                        AgentTurnEvent::Progress { entry: prior, .. } if prior.key == entry.key)
                    })
                {
                    if let Some(delta) = entry.item["text"].as_str() {
                        if let Some(Value::String(text)) = prior.item.get_mut("text") {
                            if text.len() + delta.len() > MAX_TEXT {
                                return false;
                            }
                            text.push_str(delta);
                            return true;
                        }
                    } else {
                        prior.clone_from(entry);
                        return true;
                    }
                }
            }
            _ => {}
        }
        false
    }

    fn text_budget(&mut self, text: &str) -> Result<(), AgentSessionError> {
        if text.len() > MAX_TEXT {
            return Err(self.display_limit());
        }
        self.byte_budget(text.len())
    }

    fn byte_budget(&mut self, bytes: usize) -> Result<(), AgentSessionError> {
        self.bytes = self.bytes.saturating_add(bytes);
        let text_limit = if self.buffering {
            MAX_BUFFERED_BYTES
        } else {
            MAX_BYTES
        };
        if self.bytes > text_limit || self.bytes + self.queued_tool_bytes > self.payload_limit() {
            return Err(self.display_limit());
        }
        Ok(())
    }

    fn payload_limit(&self) -> usize {
        if self.buffering || self.backlog_capacity > MAX_EVENTS {
            MAX_BUFFERED_BYTES
        } else {
            MAX_BYTES
        }
    }

    fn replace_tool_progress(&mut self, key: &str, bytes: usize) {
        self.events.retain(|event| {
            !matches!(event,
            AgentTurnEvent::Progress { entry, .. } if entry.key == key)
        });
        let previous = self
            .queued_tools
            .insert(key.to_owned(), bytes)
            .unwrap_or_default();
        self.queued_tool_bytes = self.queued_tool_bytes - previous + bytes;
    }

    fn tool_item(
        &mut self,
        suffix: &str,
        item: Value,
        bytes: usize,
        done: bool,
    ) -> Result<(), AgentSessionError> {
        let entry = self.entry(suffix, item);
        let previous = self
            .queued_tools
            .get(&entry.key)
            .copied()
            .unwrap_or_default();
        if self.bytes + self.queued_tool_bytes - previous + bytes > self.payload_limit() {
            return Err(self.display_limit());
        }
        self.replace_tool_progress(&entry.key, bytes);
        if done {
            self.push(AgentTurnEvent::Timeline(entry))
        } else {
            self.observation += 1;
            self.push(AgentTurnEvent::Progress {
                observation: format!("{}:{}", self.turn, self.observation),
                entry,
            })
        }
    }

    fn progress(&mut self, suffix: &str, item: Value) -> Result<(), AgentSessionError> {
        self.observation += 1;
        self.push(AgentTurnEvent::Progress {
            observation: format!("{}:{}", self.turn, self.observation),
            entry: self.entry(suffix, item),
        })
    }

    /// Translate one record, returning true only after terminal settlement.
    /// Malformed records and exceeded display budgets return `Failed`.
    pub(super) fn consume(&mut self, event: &Value) -> Result<bool, AgentSessionError> {
        match event["type"].as_str() {
            Some("agent_start") => {
                // An extension may finish one run and start another before acknowledging its command.
                self.resume_running();
            }
            Some("message_start") if event["message"]["role"] == "assistant" => {
                self.message += 1;
                if self.message > MAX_ITEMS {
                    return Err(AgentSessionError::Failed);
                }
            }
            Some("message_update") => self.delta(&event["assistantMessageEvent"])?,
            Some("message_end") if event["message"]["role"] == "assistant" => {
                self.message_end(&event["message"])?;
            }
            Some("tool_execution_start" | "tool_execution_update" | "tool_execution_end") => {
                self.tool(event)?;
            }
            Some("compaction_start" | "compaction_end") => self.compact(event, true)?,
            Some("agent_settled") => {
                if self.settled {
                    return Ok(true);
                }
                let aborted = event["aborted"]
                    .as_bool()
                    .ok_or(AgentSessionError::Failed)?;
                self.running = false;
                self.settled = true;
                if !self.tools.is_empty() {
                    self.failure
                        .get_or_insert(Diagnostic::new(Category::Protocol, "unfinished_tools"));
                    self.finish_tools();
                }
                self.finish_compaction();
                self.events.push_back(if aborted {
                    AgentTurnEvent::Cancelled
                } else if self.failure.is_some() {
                    AgentTurnEvent::Failed
                } else {
                    AgentTurnEvent::Completed(Some(self.last_text.clone()))
                });
                return Ok(true);
            }
            Some("response") | None => return Err(AgentSessionError::Failed),
            // agent_end does not settle automatic retries or overflow recovery.
            Some(_) => {}
        }
        Ok(false)
    }

    /// Finish an extension command only after an idle native-state barrier with no open run.
    pub(super) fn handled(&mut self) -> Result<(), AgentSessionError> {
        self.settled = true;
        self.push(AgentTurnEvent::Completed(None))
    }

    /// Project native compaction markers, including operations arriving while idle.
    /// Invalid reasons or exhausted lifecycle/event budgets return failure.
    pub(super) fn compact(
        &mut self,
        event: &Value,
        in_turn: bool,
    ) -> Result<(), AgentSessionError> {
        let trigger = match event["reason"].as_str() {
            Some("manual") => "manual",
            Some("threshold" | "overflow") => "auto",
            _ => return Err(AgentSessionError::Failed),
        };
        if event["type"] == "compaction_start" {
            if self.compaction.is_some() {
                return Ok(());
            }
            let operation = Compaction {
                id: Uuid::new_v4().to_string(),
                turn: in_turn.then(|| self.turn.clone()),
            };
            let item = self
                .compactions
                .marker(&operation.id, "loading", Some(trigger))?;
            if let Some(item) = item {
                self.push_compaction(&operation, item)?;
            }
            self.compaction = Some(operation);
        } else if let Some(operation) = self.compaction.take() {
            let failed = event["errorMessage"].is_string()
                || event["aborted"] == true
                || !event["result"].is_object();
            let status = if failed { "failed" } else { "completed" };
            if let Some(item) = self
                .compactions
                .marker(&operation.id, status, Some(trigger))?
            {
                // The terminal marker consumes the slot reserved when its start was admitted.
                self.events
                    .push_back(AgentTurnEvent::Timeline(Self::compaction_entry(
                        operation.turn.as_deref(),
                        item,
                    )));
            }
        }
        Ok(())
    }

    fn compaction_entry(turn: Option<&str>, item: Value) -> NativeItem {
        let id = item["compactionId"]
            .as_str()
            .expect("native compaction identity");
        NativeItem {
            key: format!(
                "native:pi:compaction:{}:{}",
                id,
                item["status"].as_str().unwrap_or("failed")
            ),
            turn_id: turn.map(str::to_owned),
            timestamp: Utc::now().to_rfc3339(),
            item,
        }
    }

    fn push_compaction(
        &mut self,
        operation: &Compaction,
        item: Value,
    ) -> Result<(), AgentSessionError> {
        self.push(AgentTurnEvent::Timeline(Self::compaction_entry(
            operation.turn.as_deref(),
            item,
        )))
    }

    fn finish_compaction(&mut self) {
        let operation = self.compaction.take();
        for item in self.compactions.fail_pending() {
            let turn = operation
                .as_ref()
                .filter(|operation| item["compactionId"].as_str() == Some(operation.id.as_str()))
                .and_then(|operation| operation.turn.as_deref());
            self.events
                .push_back(AgentTurnEvent::Timeline(Self::compaction_entry(turn, item)));
        }
    }

    fn delta(&mut self, event: &Value) -> Result<(), AgentSessionError> {
        let kind = match event["type"].as_str() {
            Some("text_delta") => "assistant_message",
            Some("thinking_delta") => "reasoning",
            _ => return Ok(()),
        };
        let text = event["delta"].as_str().ok_or(AgentSessionError::Failed)?;
        self.text_budget(text)?;
        let index = event["contentIndex"]
            .as_u64()
            .ok_or(AgentSessionError::Failed)?;
        let suffix = format!("message:{}:{index}", self.message);
        let key = format!("native:pi:{}:{suffix}", self.turn);
        self.progress(&suffix, json!({"type":kind,"messageId":key,"text":text}))
    }

    fn message_end(&mut self, message: &Value) -> Result<(), AgentSessionError> {
        let blocks = message["content"]
            .as_array()
            .filter(|blocks| blocks.len() <= MAX_ITEMS)
            .ok_or(AgentSessionError::Failed)?;
        self.failure = (message["stopReason"] == "error").then(|| {
            Diagnostic::new(
                classify(message["errorMessage"].as_str().unwrap_or_default()),
                "message_end",
            )
        });
        self.explanation.clear();
        if let Some(diagnostic) = self.failure {
            self.explanation
                .observe("Pi", &message["errorMessage"], diagnostic);
        }
        self.last_text.clear();
        for (index, block) in blocks.iter().enumerate() {
            let (kind, field) = match block["type"].as_str() {
                Some("text") => ("assistant_message", "text"),
                Some("thinking") => ("reasoning", "thinking"),
                _ => continue,
            };
            let text = block[field].as_str().ok_or(AgentSessionError::Failed)?;
            self.text_budget(text)?;
            if self.last_text.len() + text.len() > MAX_TEXT {
                return Err(self.display_limit());
            }
            if kind == "assistant_message" {
                self.last_text.push_str(text);
            }
            // Pi contentIndex must identify this same block throughout streaming;
            // final text must retain the prefix already published for that key.
            let suffix = format!("message:{}:{index}", self.message);
            let key = format!("native:pi:{}:{suffix}", self.turn);
            self.push(AgentTurnEvent::Timeline(
                self.entry(&suffix, json!({"type":kind,"messageId":key,"text":text})),
            ))?;
        }
        if let Some(usage) = message.get("usage") {
            let snapshot = AgentUsage {
                input_tokens: usage["input"].as_u64(),
                cached_input_tokens: usage["cacheRead"].as_u64(),
                output_tokens: usage["output"].as_u64(),
                total_cost_usd: usage["cost"]["total"].as_f64(),
                ..AgentUsage::default()
            };
            if !snapshot.is_valid() {
                return Err(AgentSessionError::Failed);
            }
            self.push(AgentTurnEvent::Usage(snapshot))?;
        }
        Ok(())
    }

    fn tool(&mut self, event: &Value) -> Result<(), AgentSessionError> {
        let id = event["toolCallId"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 512)
            .ok_or(AgentSessionError::Failed)?;
        // Updates can omit display metadata; the start remains authoritative.
        let name = event["toolName"]
            .as_str()
            .filter(|name| !name.is_empty() && name.len() <= 512)
            .unwrap_or("unknown");
        let done = event["type"] == "tool_execution_end";
        if event["type"] == "tool_execution_start" {
            if self.tools.len() >= MAX_TOOLS
                || self.tools.contains_key(id)
                || self
                    .queued_tools
                    .contains_key(&format!("native:pi:{}:tool:{id}", self.turn))
                || self.events.len() + self.tools.len() + 3 >= self.event_limit()
            {
                return Err(self.display_limit());
            }
            let command = (name == "bash")
                .then(|| event["args"]["command"].as_str())
                .flatten();
            let (input, input_bytes) = if let Some(edit) = file_preview(name, &event["args"]) {
                edit
            } else {
                preview_with_size(command.map_or(&event["args"], |_| &event["args"]["command"]))
            };
            self.tools.insert(
                id.to_owned(),
                Tool {
                    shell: command.is_some(),
                    name: name.to_owned(),
                    input,
                    input_bytes,
                },
            );
        }
        let tool = self.tools.get(id).ok_or(AgentSessionError::Failed)?;
        let name = &tool.name;
        let result = if done {
            &event["result"]
        } else {
            &event["partialResult"]
        };
        let (output, output_bytes) = if tool.shell {
            preview_with_size(&shell_output(result))
        } else if tool.input["type"] == "edit" && tool.name == "edit" {
            let patch = &result["details"]["patch"];
            if patch.is_string() {
                preview_with_size(patch)
            } else {
                (Value::Null, 0)
            }
        } else if tool.input["type"] == "write" && tool.name == "write" {
            (Value::Null, 0)
        } else if tool.input["type"] == "read" && tool.name == "read" {
            preview_with_size(&shell_output(result))
        } else {
            preview_with_size(result)
        };
        let preview_bytes = tool.input_bytes + output_bytes;
        let status = if done {
            if event["isError"] == true {
                "failed"
            } else {
                "completed"
            }
        } else {
            "running"
        };
        let suffix = format!("tool:{id}");
        let item = json!({"type":"tool_call","callId":format!("native:pi:{}:{suffix}",self.turn),
            "name":name,"status":status,"error":if status == "failed" { json!("Tool execution failed.") } else { Value::Null },"detail":tool_detail(tool, output)});
        self.tool_item(&suffix, item, preview_bytes, done)?;
        if done {
            self.tools.remove(id);
        }
        Ok(())
    }

    fn finish_tools(&mut self) {
        for (id, tool) in std::mem::take(&mut self.tools) {
            let suffix = format!("tool:{id}");
            let entry = self.entry(&suffix,
                json!({"type":"tool_call","callId":format!("native:pi:{}:{suffix}",self.turn),
                    "name":tool.name,"status":"failed","error":"Tool execution did not complete.","detail":tool_detail(&tool, Value::Null)}));
            // Cleanup may use reserved capacity; its input preview was already bounded at start.
            self.replace_tool_progress(&entry.key, tool.input_bytes);
            self.events.push_back(AgentTurnEvent::Timeline(entry));
        }
    }

    /// Retire pending tools/compactions within reserved queue capacity and fail once.
    pub(super) fn fail(&mut self) {
        self.events.retain(|event| {
            !matches!(
                event,
                AgentTurnEvent::Completed(_) | AgentTurnEvent::Cancelled | AgentTurnEvent::Failed
            )
        });
        self.finish_tools();
        self.finish_compaction();
        self.events.push_back(AgentTurnEvent::Failed);
        self.settled = true;
        self.running = false;
    }
}

/// Pi display suffix for output retained in the native transcript.
const PREVIEW_TRUNCATION_MARKER: &str =
    "\n[Output truncated; full output remains in the native transcript.]";

/// Return a preview and its displayed byte count, reusing the rendering needed for truncation.
/// The count excludes JSON escaping/framing for strings and includes the truncation marker.
pub(super) fn preview_with_size(value: &Value) -> (Value, usize) {
    const MAX_PREVIEW: usize = 32 * 1024;
    let text = value
        .as_str()
        .map_or_else(|| Cow::Owned(value.to_string()), Cow::Borrowed);
    if text.len() <= MAX_PREVIEW {
        return (value.clone(), text.len());
    }
    let end = text.floor_char_boundary(MAX_PREVIEW);
    let preview = format!("{}{PREVIEW_TRUNCATION_MARKER}", &text[..end]);
    let bytes = preview.len();
    (json!(preview), bytes)
}

fn tool_detail(tool: &Tool, output: Value) -> Value {
    if tool.shell {
        let mut detail = json!({"type":"shell","command":tool.input});
        if output.is_string() {
            detail["output"] = output;
        }
        detail
    } else if tool.name == "edit" && tool.input["type"] == "edit" {
        let mut detail = tool.input.clone();
        if output.is_string() {
            detail["unifiedDiff"] = output;
        }
        detail
    } else if tool.name == "write" && tool.input["type"] == "write" {
        tool.input.clone()
    } else if tool.name == "read" && tool.input["type"] == "read" {
        let mut detail = tool.input.clone();
        if output.is_string() {
            detail["content"] = output;
        }
        detail
    } else {
        json!({"type":"unknown","input":tool.input,"output":output})
    }
}

fn shell_output(result: &Value) -> Value {
    if result.is_string() || result.is_null() {
        return result.clone();
    }
    let Some(content) = result["content"].as_array() else {
        return Value::String(result.to_string());
    };
    let mut output = String::new();
    for text in content
        .iter()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str())
    {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(text);
    }
    if output.is_empty() && !content.is_empty() {
        return Value::String(result.to_string());
    }
    Value::String(output)
}

fn file_preview(name: &str, args: &Value) -> Option<(Value, usize)> {
    if !matches!(name, "edit" | "write" | "read") {
        return None;
    }
    args["path"].as_str()?;
    let (path, mut bytes) = preview_with_size(&args["path"]);
    let mut detail = json!({"type":name,"filePath":path});
    if name == "read" {
        for field in ["offset", "limit"] {
            if let Some(value) = args[field].as_u64() {
                detail[field] = json!(value);
            }
        }
        return Some((detail, bytes));
    }
    if name == "write" {
        args["content"].as_str()?;
        let (content, size) = preview_with_size(&args["content"]);
        detail["content"] = content;
        return Some((detail, bytes + size));
    }
    if let Some(edits) = args["edits"].as_array() {
        let (diff, size) = edit_array_preview(edits)?;
        detail["unifiedDiff"] = diff;
        return Some((detail, bytes + size));
    }
    for (source, target) in [("oldText", "oldString"), ("newText", "newString")] {
        args[source].as_str()?;
        let (preview, size) = preview_with_size(&args[source]);
        detail[target] = preview;
        bytes += size;
    }
    Some((detail, bytes))
}

fn edit_array_preview(edits: &[Value]) -> Option<(Value, usize)> {
    if edits.is_empty() {
        return None;
    }
    let mut diff = String::new();
    for edit in edits {
        let old = edit["oldText"].as_str()?;
        let new = edit["newText"].as_str()?;
        // Without the file, each pending replacement is an independent snippet.
        // Completion replaces this preview with Pi's patch and actual file ranges.
        diff.push_str("@@ @@\n");
        for (prefix, text) in [('-', old), ('+', new)] {
            for line in text.lines() {
                diff.push(prefix);
                diff.push_str(line);
                diff.push('\n');
            }
        }
    }
    Some(preview_with_size(&Value::String(diff)))
}
