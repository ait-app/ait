//! Terminal input is a display event; the native PTY owns execution and process lifetime.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::ports::agent_session::{AgentSessionError, AgentTurnEvent};
use crate::protocol::timeline::NativeItem;

const MAX_COMMANDS: usize = 128;
const PREVIEW_BYTES: usize = 16 * 1024;

/// Bounded terminal labels retained across turns in one native session.
#[derive(Debug, Default)]
pub(super) struct Terminal {
    commands: BTreeMap<String, String>,
    ordinal: u64,
}

impl Terminal {
    /// Observe process labels or project stdin without treating it as assistant output.
    /// Returns a protocol error for malformed terminal interaction identifiers or input.
    pub(super) fn observe(
        &mut self,
        method: &str,
        params: &Value,
    ) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        if matches!(method, "item/started" | "item/completed")
            && params["item"]["type"] == "commandExecution"
        {
            let item = &params["item"];
            if let (Some(process), Some(command)) =
                (item["processId"].as_str(), item["command"].as_str())
                && !process.is_empty()
                && process.len() <= 1024
                && command.len() <= PREVIEW_BYTES
            {
                if self.commands.len() >= MAX_COMMANDS && !self.commands.contains_key(process) {
                    self.commands.pop_first();
                }
                self.commands.insert(process.to_owned(), command.to_owned());
            }
        }
        if method != "item/commandExecution/terminalInteraction" {
            return Ok(None);
        }
        let process = params["processId"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 1024)
            .ok_or(AgentSessionError::Failed)?;
        let turn = params["turnId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or(AgentSessionError::Failed)?;
        let input = params["stdin"].as_str().ok_or(AgentSessionError::Failed)?;
        self.ordinal = self
            .ordinal
            .checked_add(1)
            .ok_or(AgentSessionError::Failed)?;
        let id = format!("terminal:{process}:{}", self.ordinal);
        let mut end = input.len().min(PREVIEW_BYTES);
        while !input.is_char_boundary(end) {
            end -= 1;
        }
        let text = if end < input.len() {
            format!("{}\n[Terminal input preview truncated]", &input[..end])
        } else {
            input.to_owned()
        };
        let mut detail = json!({"type":"plain_text", "text":text, "icon":"square_terminal"});
        if let Some(command) = self.commands.get(process) {
            detail["label"] = json!(command);
        }
        Ok(Some(AgentTurnEvent::Timeline(NativeItem {
            key: format!("native:{turn}:{id}"),
            turn_id: Some(turn.to_owned()),
            timestamp: super::discovery::timestamp(),
            item: json!({"type":"tool_call", "callId":id, "name":"terminal", "status":"completed",
                "error":null, "detail":detail, "metadata":{"processId":process}}),
        })))
    }
}

#[cfg(test)]
mod tests;
