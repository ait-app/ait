use std::borrow::Cow;
use std::collections::{BTreeMap, VecDeque};

use chrono::Utc;
use serde_json::{Value, json};

use crate::ports::agent_session::{AgentSessionError, AgentTurnEvent};
use crate::protocol::{timeline::NativeItem, usage::AgentUsage};

const MAX_TEXT: usize = 128 * 1024;
const MAX_ITEMS: usize = 4096;

#[derive(Debug)]
struct Step {
    entry: NativeItem,
    text: String,
    tool: Value,
    done: bool,
}

#[derive(Debug, Default)]
pub(super) struct Stream {
    pub(super) events: VecDeque<AgentTurnEvent>,
    steps: BTreeMap<u64, Step>,
    turn: String,
    conversation: String,
    observation: usize,
    bytes: usize,
    assistant_seen: bool,
}

impl Stream {
    pub(super) fn begin(&mut self, turn: String, conversation: String) {
        self.steps.clear();
        self.turn = turn;
        self.conversation = conversation;
        self.observation = 0;
        self.bytes = 0;
        self.assistant_seen = false;
    }

    pub(super) fn update(&mut self, update: &Value) -> Result<(), AgentSessionError> {
        if update["conversation_id"] != self.conversation {
            return Err(AgentSessionError::Failed);
        }
        let done = match update["state"].as_str() {
            Some("ACTIVE") => false,
            Some("DONE") => true,
            _ => return Err(AgentSessionError::Failed),
        };
        let kind = match update["step_type"].as_str() {
            Some("agent_response") => "assistant_message",
            Some("tool") => "tool_call",
            // Native user/checkpoint and new categories do not change the host's lifecycle.
            Some(_) => return Ok(()),
            None => return Err(AgentSessionError::Failed),
        };
        let index = update["step_index"]
            .as_u64()
            .ok_or(AgentSessionError::Failed)?;
        if self.steps.len() >= MAX_ITEMS && !self.steps.contains_key(&index) {
            return Err(AgentSessionError::Failed);
        }
        let key = format!(
            "native:antigravity:{}:{}:step:{index}",
            self.conversation, self.turn
        );
        let step = self.steps.entry(index).or_insert_with(|| Step {
            entry: NativeItem {
                key: key.clone(),
                turn_id: Some(self.turn.clone()),
                timestamp: Utc::now().to_rfc3339(),
                item: json!({"type":kind,"messageId":key}),
            },
            text: String::new(),
            tool: json!({}),
            done: false,
        });
        if step.done || step.entry.item["type"] != kind {
            return Err(AgentSessionError::Failed);
        }
        let mut progress = step.entry.clone();
        if kind == "assistant_message" {
            self.assistant_seen = true;
            let delta = update
                .get("text_delta")
                .map(|value| value.as_str().ok_or(AgentSessionError::Failed))
                .transpose()?
                .unwrap_or_default();
            if step.text.len().saturating_add(delta.len()) > MAX_TEXT {
                return Err(AgentSessionError::Failed);
            }
            step.text.push_str(delta);
            self.bytes = self.bytes.saturating_add(delta.len());
            step.entry.item["text"] = json!(step.text);
            progress.item["text"] = json!(delta);
        } else {
            if let Some(info) = update.get("tool_info") {
                let fields = info.as_object().ok_or(AgentSessionError::Failed)?;
                for (name, value) in fields {
                    let (value, bytes) = preview(value);
                    self.bytes = self.bytes.saturating_add(bytes);
                    step.tool[name] = value;
                }
            }
            if let Some(name) = update.get("tool_name") {
                step.tool["name"] = name.clone();
            }
            let name = step.tool["name"]
                .as_str()
                .filter(|name| {
                    !name.is_empty() && name.len() <= 512 && !name.chars().any(char::is_control)
                })
                .ok_or(AgentSessionError::Failed)?;
            let status = tool_status(&step.tool, done);
            step.entry.item = json!({"type":"tool_call","callId":key,"name":name,"status":status,
                "detail":tool_detail(&step.tool),"error":step.tool["error"]});
            if let Some(info) = update.get("subagent_info") {
                let (info, bytes) = preview(info);
                self.bytes = self.bytes.saturating_add(bytes);
                step.entry.item["metadata"] = json!({"subagentInfo":info});
            }
            progress.clone_from(&step.entry);
        }
        if self.bytes > 8 * 1024 * 1024 {
            return Err(AgentSessionError::Failed);
        }
        self.observation += 1;
        if done {
            step.done = true;
            self.events
                .push_back(AgentTurnEvent::Timeline(step.entry.clone()));
        } else {
            self.events.push_back(AgentTurnEvent::Progress {
                observation: format!("{}:{}", self.turn, self.observation),
                entry: progress,
            });
        }
        Ok(())
    }

    pub(super) fn finish(&mut self, result: &Value) -> Result<(), AgentSessionError> {
        if result["conversation_id"] != self.conversation {
            return Err(AgentSessionError::Failed);
        }
        let status = result["status"].as_str().ok_or(AgentSessionError::Failed)?;
        let response = result["response"]
            .as_str()
            .ok_or(AgentSessionError::Failed)?;
        if response.len() > MAX_TEXT {
            return Err(AgentSessionError::Failed);
        }
        if let Some(usage) = result.get("usage") {
            let snapshot = AgentUsage {
                input_tokens: counter(usage, "input_tokens")?,
                cached_input_tokens: counter(usage, "cache_read_tokens")?,
                output_tokens: counter(usage, "output_tokens")?,
                ..AgentUsage::default()
            };
            if !snapshot.is_valid() {
                return Err(AgentSessionError::Failed);
            }
            // Native result usage is cumulative. Publish replacement snapshots, never sums.
            self.events.push_back(AgentTurnEvent::Usage(snapshot));
        }
        if status == "SUCCESS" && self.steps.values().any(|step| !step.done) {
            return Err(AgentSessionError::Failed);
        }
        if !self.assistant_seen && !response.is_empty() {
            let key = format!(
                "native:antigravity:{}:result:{}",
                self.conversation, self.turn
            );
            self.events.push_back(AgentTurnEvent::Timeline(NativeItem {
                key: key.clone(),
                turn_id: Some(self.turn.clone()),
                timestamp: Utc::now().to_rfc3339(),
                item: json!({"type":"assistant_message","messageId":key,"text":response}),
            }));
        }
        self.events.push_back(match status {
            "SUCCESS" => AgentTurnEvent::Completed(Some(response.to_owned())),
            "CANCELED" | "INTERRUPTED" => AgentTurnEvent::Cancelled,
            _ => AgentTurnEvent::Failed,
        });
        Ok(())
    }
}

fn counter(usage: &Value, field: &str) -> Result<Option<u64>, AgentSessionError> {
    usage
        .get(field)
        .map(|value| value.as_u64().ok_or(AgentSessionError::Failed))
        .transpose()
}

fn preview(value: &Value) -> (Value, usize) {
    const MAX_PREVIEW: usize = 32 * 1024;
    let encoded = value
        .as_str()
        .map_or_else(|| Cow::Owned(value.to_string()), Cow::Borrowed);
    if encoded.len() <= MAX_PREVIEW {
        return (value.clone(), encoded.len());
    }
    let mut end = MAX_PREVIEW;
    while !encoded.is_char_boundary(end) {
        end -= 1;
    }
    (
        json!(format!(
            "{}\n[Output truncated; full output remains in AGY.]",
            &encoded[..end]
        )),
        end,
    )
}

fn tool_detail(tool: &Value) -> Value {
    let parameters = &tool["parameters"];
    match tool["name"].as_str() {
        Some("run_command") if parameters["CommandLine"].is_string() => {
            let mut detail = json!({"type":"shell","command":parameters["CommandLine"],
                "output":tool["output"]});
            if let Some(cwd) = parameters.get("Cwd") {
                detail["cwd"] = cwd.clone();
            }
            detail
        }
        _ => json!({"type":"unknown","input":parameters,"output":tool["output"]}),
    }
}

fn tool_status(tool: &Value, done: bool) -> &'static str {
    if !done {
        "running"
    } else if tool["error"].is_null() {
        "completed"
    } else {
        "failed"
    }
}

#[cfg(test)]
mod tests;
