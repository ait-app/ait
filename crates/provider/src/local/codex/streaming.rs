use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use uuid::Uuid;

use super::discovery;
use crate::ports::agent_session::{AgentSessionError, AgentTurnEvent};
use crate::protocol::timeline::NativeItem;

const MAX_ITEMS: usize = 4096;
const MAX_TEXT: usize = 192 * 1024;
const MAX_OUTPUT: usize = 16 * 1024;

pub(super) fn steer_rejected(error: &Value) -> bool {
    if error["code"] == -32601 {
        return true;
    }
    if error["code"] != -32600 {
        return false;
    }
    if error
        .pointer("/data/codexErrorInfo/activeTurnNotSteerable")
        .is_some_and(Value::is_object)
    {
        return true;
    }
    let Some(message) = error["message"].as_str() else {
        return false;
    };
    if matches!(
        message,
        "no active turn to steer" | "active turn uses a different output schema"
    ) {
        return true;
    }
    message
        .strip_prefix("expected active turn id `")
        .and_then(|message| message.strip_suffix('`'))
        .and_then(|message| message.split_once("` but found `"))
        .is_some_and(|(expected, actual)| {
            !expected.is_empty()
                && !actual.is_empty()
                && !expected.contains('`')
                && !actual.contains('`')
        })
}

#[derive(Debug, Default)]
pub(super) struct Stream {
    pub(super) events: std::collections::VecDeque<AgentTurnEvent>,
    completed: BTreeSet<String>,
    compactions: BTreeSet<String>,
    anonymous_compactions: usize,
    item_compactions: usize,
    text_bytes: BTreeMap<String, usize>,
    summary_indices: BTreeMap<String, u64>,
    tools: BTreeMap<String, Vec<(String, Value)>>,
    tool_rows: usize,
}

impl Stream {
    pub(super) fn complete(&mut self, item: &Value) -> Result<bool, AgentSessionError> {
        let id = text(item, "id")?;
        if self.completed.contains(id) {
            return Ok(false);
        }
        if self.completed.len() >= MAX_ITEMS {
            tracing::warn!(
                limit_items = MAX_ITEMS,
                "Codex completed item limit reached"
            );
            return Err(AgentSessionError::Failed);
        }
        self.text_bytes.remove(id);
        let pending_compaction = self.compactions.remove(id);
        let duplicate = item["type"] == "contextCompaction"
            && !pending_compaction
            && self.anonymous_compactions > 0;
        if item["type"] == "contextCompaction" {
            if duplicate {
                self.anonymous_compactions -= 1;
            } else {
                self.item_compactions += 1;
            }
        }
        self.summary_indices.remove(id);
        if let Some(items) = self.tools.remove(id) {
            self.tool_rows -= items.len();
        }
        Ok(self.completed.insert(id.to_owned()) && !duplicate)
    }

    /// Settle loading compactions before the native turn's terminal event.
    /// Returns an error if the completed-item budget is exhausted.
    pub(super) fn finish_compactions(&mut self, turn: &str) -> Result<(), AgentSessionError> {
        for id in std::mem::take(&mut self.compactions) {
            if self.complete(&json!({"type":"contextCompaction","id":id}))? {
                self.events.push_back(compaction_entry(Some(turn), &id));
            }
        }
        Ok(())
    }

    /// Pair the legacy notification with native item completion in either arrival order.
    /// Returns an error if retaining the native compaction identity fails.
    pub(super) fn compacted(
        &mut self,
        turn: Option<&str>,
    ) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        if self.item_compactions > 0 {
            self.item_compactions -= 1;
            return Ok(None);
        }
        let id = if let Some(id) = self.compactions.first().cloned() {
            if self.completed.len() >= MAX_ITEMS {
                return Err(AgentSessionError::Failed);
            }
            self.compactions.remove(&id);
            self.completed.insert(id.clone());
            id
        } else {
            self.anonymous_compactions += 1;
            Uuid::new_v4().to_string()
        };
        Ok(Some(compaction_entry(turn, &id)))
    }

    pub(super) fn progress(
        &mut self,
        method: &str,
        params: &Value,
    ) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        let item = match method {
            "item/agentMessage/delta" | "item/reasoning/summaryTextDelta" => {
                self.text_delta(method, params)?
            }
            "item/started" if params["item"]["type"] == "contextCompaction" => {
                return self.compaction_started(params);
            }
            "item/started" => return self.tool_started(params),
            "item/commandExecution/outputDelta" | "item/fileChange/outputDelta" => {
                return self.tool_output(params);
            }
            _ => None,
        };
        let Some((id, item)) = item else {
            return Ok(None);
        };
        let turn = text(params, "turnId")?;
        Ok(Some(AgentTurnEvent::Progress {
            observation: Uuid::new_v4().to_string(),
            entry: NativeItem {
                key: format!("native:{turn}:{id}"),
                turn_id: Some(turn.to_owned()),
                timestamp: discovery::timestamp(),
                item,
            },
        }))
    }

    fn compaction_started(
        &mut self,
        params: &Value,
    ) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        let id = text(&params["item"], "id")?;
        let turn = text(params, "turnId")?;
        if self.completed.contains(id) || self.compactions.contains(id) {
            return Ok(None);
        }
        if self.compactions.len() >= MAX_ITEMS {
            return Err(AgentSessionError::Failed);
        }
        self.compactions.insert(id.to_owned());
        Ok(Some(AgentTurnEvent::Timeline(NativeItem {
            key: format!("native:{turn}:{id}:loading"),
            turn_id: Some(turn.to_owned()),
            timestamp: discovery::timestamp(),
            item: json!({"type":"compaction","status":"loading"}),
        })))
    }

    fn text_delta(
        &mut self,
        method: &str,
        params: &Value,
    ) -> Result<Option<(String, Value)>, AgentSessionError> {
        let id = text(params, "itemId")?;
        let delta = params["delta"].as_str().ok_or(AgentSessionError::Failed)?;
        if self.completed.contains(id) || delta.is_empty() {
            return Ok(None);
        }
        if self.text_bytes.len() >= MAX_ITEMS && !self.text_bytes.contains_key(id) {
            tracing::warn!(
                limit_items = MAX_ITEMS,
                "Codex streaming text item limit reached"
            );
            return Err(AgentSessionError::Failed);
        }
        let reasoning = method == "item/reasoning/summaryTextDelta";
        let mut prefix = "";
        if reasoning {
            let index = params["summaryIndex"]
                .as_u64()
                .ok_or(AgentSessionError::Failed)?;
            let previous = self.summary_indices.entry(id.to_owned()).or_insert(0);
            if index < *previous || index > *previous + 1 {
                return Err(AgentSessionError::Failed);
            }
            if index > *previous {
                prefix = "\n";
            }
            *previous = index;
        }
        let bytes = self.text_bytes.entry(id.to_owned()).or_default();
        *bytes = bytes.saturating_add(delta.len() + prefix.len());
        if *bytes > MAX_TEXT {
            tracing::warn!(
                size_bytes = *bytes,
                limit_bytes = MAX_TEXT,
                "Codex streaming text exceeds size limit"
            );
            return Err(AgentSessionError::Failed);
        }
        let mut item = json!({"type":if reasoning {"reasoning"} else {"assistant_message"},
            "text":format!("{prefix}{delta}")});
        if !reasoning {
            item["messageId"] = json!(id);
        }
        Ok(Some((id.to_owned(), item)))
    }

    fn tool_started(
        &mut self,
        params: &Value,
    ) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        let native = &params["item"];
        let kind = text(native, "type")?;
        if !matches!(
            kind,
            "commandExecution"
                | "fileChange"
                | "mcpToolCall"
                | "dynamicToolCall"
                | "webSearch"
                | "collabAgentToolCall"
                | "subAgentActivity"
        ) {
            return Ok(None);
        }
        let id = text(native, "id")?;
        if self.completed.contains(id) || self.tools.contains_key(id) {
            return Ok(None);
        }
        if self.tool_rows >= MAX_ITEMS {
            tracing::warn!(
                limit_items = MAX_ITEMS,
                "Codex running tool item limit reached"
            );
            return Err(AgentSessionError::Failed);
        }
        let turn = text(params, "turnId")?;
        let items = crate::local::tool_detail::codex_tools(native, "running");
        if items.len() > MAX_ITEMS - self.tool_rows {
            tracing::warn!(
                running_items = self.tool_rows,
                new_items = items.len(),
                limit_items = MAX_ITEMS,
                "Codex running tool item limit reached"
            );
            return Err(AgentSessionError::Failed);
        }
        self.tool_rows += items.len();
        self.tools.insert(id.to_owned(), items.clone());
        Ok(self.tool_events(turn, items))
    }

    fn tool_output(&mut self, params: &Value) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        let id = text(params, "itemId")?;
        let delta = params["delta"].as_str().ok_or(AgentSessionError::Failed)?;
        // Compound actions append one command result; a single action owns its own output.
        let Some((call_id, item)) = self.tools.get_mut(id).and_then(|items| items.last_mut())
        else {
            return Ok(None);
        };
        if delta.is_empty() {
            return Ok(None);
        }
        let turn = text(params, "turnId")?;
        let field = output_field(item);
        let output = item["detail"][field].as_str().unwrap_or_default();
        // The live preview is a bounded tail. Complete native output remains in the final item.
        let mut tail = String::with_capacity(MAX_OUTPUT.min(output.len() + delta.len()));
        if delta.len() < MAX_OUTPUT {
            tail.push_str(output);
        }
        let mut start = delta.len().saturating_sub(MAX_OUTPUT);
        while !delta.is_char_boundary(start) {
            start += 1;
        }
        tail.push_str(&delta[start..]);
        let mut start = tail.len().saturating_sub(MAX_OUTPUT);
        while !tail.is_char_boundary(start) {
            start += 1;
        }
        if start > 0 {
            tail.drain(..start);
        }
        item["detail"][field] = json!(tail);
        let item = (call_id.clone(), item.clone());
        Ok(self.tool_events(turn, vec![item]))
    }

    fn tool_events(&mut self, turn: &str, items: Vec<(String, Value)>) -> Option<AgentTurnEvent> {
        let timestamp = discovery::timestamp();
        let mut events = items
            .into_iter()
            .map(|(id, item)| AgentTurnEvent::Progress {
                observation: Uuid::new_v4().to_string(),
                entry: NativeItem {
                    key: format!("native:{turn}:{id}"),
                    turn_id: Some(turn.to_owned()),
                    timestamp: timestamp.clone(),
                    item,
                },
            });
        let first = events.next();
        self.events.extend(events);
        first
    }
}

fn output_field(item: &Value) -> &'static str {
    match item["detail"]["type"].as_str() {
        Some("read" | "search") => "content",
        _ => "output",
    }
}

fn compaction_entry(turn: Option<&str>, id: &str) -> AgentTurnEvent {
    AgentTurnEvent::Timeline(NativeItem {
        key: format!("native:{}:{id}", turn.unwrap_or("codex:compaction")),
        turn_id: turn.map(str::to_owned),
        timestamp: discovery::timestamp(),
        item: json!({"type":"compaction","status":"completed"}),
    })
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, AgentSessionError> {
    value[key]
        .as_str()
        .filter(|value| !value.is_empty() && value.len() <= 1024)
        .ok_or(AgentSessionError::Failed)
}

#[cfg(test)]
mod tests;
