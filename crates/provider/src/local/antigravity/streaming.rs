use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use chrono::Utc;
use serde_json::{Value, json};

use super::diagnostics::Failure;
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
    published: bool,
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
    pub(super) failure: Option<Failure>,
}

impl Stream {
    pub(super) fn begin(&mut self, turn: String, conversation: String) {
        self.steps.clear();
        self.turn = turn;
        self.conversation = conversation;
        self.observation = 0;
        self.bytes = 0;
        self.assistant_seen = false;
        self.failure = None;
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
            published: false,
        });
        if step.done || step.entry.item["type"] != kind {
            return Err(AgentSessionError::Failed);
        }
        let mut progress = step.entry.clone();
        if kind == "assistant_message" {
            let delta = update
                .get("text_delta")
                .map(|value| value.as_str().ok_or(AgentSessionError::Failed))
                .transpose()?
                .unwrap_or_default();
            self.assistant_seen |= !delta.is_empty();
            if step.text.len().saturating_add(delta.len()) > MAX_TEXT {
                return Err(AgentSessionError::Failed);
            }
            step.text.push_str(delta);
            self.bytes = self.bytes.saturating_add(delta.len());
            step.entry.item["text"] = json!(step.text);
            progress.item["text"] = json!(delta);
        } else {
            self.bytes = self.bytes.saturating_add(update_tool(step, update, done)?);
            progress.clone_from(&step.entry);
        }
        if self.bytes > 8 * 1024 * 1024 {
            return Err(AgentSessionError::Failed);
        }
        self.observation += 1;
        if done {
            step.done = true;
            // AGY omits both output and error for auto-denied tools. Wait for denied_actions
            // before committing an immutable completion for an otherwise empty tool result.
            if kind == "tool_call" && step.tool["output"].is_null() && step.tool["error"].is_null()
            {
                progress.item["status"] = json!("running");
                self.events.push_back(AgentTurnEvent::Progress {
                    observation: format!("{}:{}", self.turn, self.observation),
                    entry: progress,
                });
            } else {
                step.published = true;
                self.events
                    .push_back(AgentTurnEvent::Timeline(step.entry.clone()));
            }
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
        let denied_tools = denied_tools(result)?;
        let denied = !denied_tools.is_empty();
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
        if status == "SUCCESS"
            && self.steps.values().any(|step| {
                !(step.done
                    || step.entry.item["type"] == "tool_call"
                        && step.tool["name"]
                            .as_str()
                            .is_some_and(|name| denied_tools.contains(&normalized_tool_name(name))))
            })
        {
            return Err(AgentSessionError::Failed);
        }
        let failed = !matches!(status, "SUCCESS" | "CANCELED" | "INTERRUPTED")
            || (denied && response.trim().is_empty());
        if failed {
            self.failure = Some(if denied {
                Failure::Permission
            } else {
                result["error"]
                    .as_str()
                    .and_then(Failure::classify)
                    .unwrap_or(Failure::Native)
            });
        }
        for step in self.steps.values_mut().filter(|step| !step.published) {
            if step.entry.item["type"] == "tool_call" {
                let refused = step.tool["name"]
                    .as_str()
                    .is_some_and(|name| denied_tools.contains(&normalized_tool_name(name)));
                if refused || !step.done {
                    let failure = if refused {
                        Failure::Permission
                    } else {
                        self.failure.unwrap_or(Failure::Exit)
                    };
                    if !refused && matches!(status, "CANCELED" | "INTERRUPTED") {
                        step.entry.item["status"] = json!("canceled");
                        step.entry.item["error"] = Value::Null;
                    } else {
                        step.entry.item["status"] = json!("failed");
                        step.entry.item["error"] = json!({"message":failure.message()});
                    }
                }
            }
            step.published = true;
            self.events
                .push_back(AgentTurnEvent::Timeline(step.entry.clone()));
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
            "SUCCESS" if failed => AgentTurnEvent::Failed,
            "SUCCESS" => AgentTurnEvent::Completed(Some(response.to_owned())),
            "CANCELED" | "INTERRUPTED" => AgentTurnEvent::Cancelled,
            _ => AgentTurnEvent::Failed,
        });
        Ok(())
    }

    pub(super) fn abort(&mut self, failure: Failure) {
        self.failure = Some(self.failure.map_or(failure, |current| current.max(failure)));
        let failure = self.failure.expect("failure assigned above");
        for step in self.steps.values_mut().filter(|step| !step.published) {
            if step.entry.item["type"] == "tool_call" {
                step.entry.item["status"] = json!("failed");
                step.entry.item["error"] = json!({"message":failure.message()});
            }
            step.published = true;
            self.events
                .push_back(AgentTurnEvent::Timeline(step.entry.clone()));
        }
        self.events.push_back(AgentTurnEvent::Failed);
    }
}

fn update_tool(step: &mut Step, update: &Value, done: bool) -> Result<usize, AgentSessionError> {
    let mut bytes = 0_usize;
    if let Some(info) = update.get("tool_info") {
        let fields = info.as_object().ok_or(AgentSessionError::Failed)?;
        for (name, value) in fields {
            let (value, size) = preview(value);
            bytes = bytes.saturating_add(size);
            step.tool[name] = value;
        }
    }
    if let Some(name) = update.get("tool_name") {
        step.tool["name"] = name.clone();
    }
    let name = step.tool["name"]
        .as_str()
        .filter(|name| !name.is_empty() && name.len() <= 512 && !name.chars().any(char::is_control))
        .ok_or(AgentSessionError::Failed)?;
    step.entry.item = json!({"type":"tool_call","callId":step.entry.key,"name":name,
        "status":tool_status(&step.tool, done),"detail":tool_detail(&step.tool),
        "error":step.tool["error"]});
    if let Some(info) = update.get("subagent_info") {
        let (info, size) = preview(info);
        bytes = bytes.saturating_add(size);
        step.entry.item["metadata"] = json!({"subagentInfo":info});
    }
    Ok(bytes)
}

fn denied_tools(result: &Value) -> Result<BTreeSet<String>, AgentSessionError> {
    let mut names = BTreeSet::new();
    let Some(actions) = result.get("denied_actions") else {
        return Ok(names);
    };
    let actions = actions.as_array().ok_or(AgentSessionError::Failed)?;
    if actions.len() > MAX_ITEMS {
        return Err(AgentSessionError::Failed);
    }
    for action in actions {
        let display = action["display_name"]
            .as_str()
            .ok_or(AgentSessionError::Failed)?;
        if display.is_empty() || display.len() > 512 || display.chars().any(char::is_control) {
            return Err(AgentSessionError::Failed);
        }
        names.insert(normalized_tool_name(display));
        if action["action"] == "command" {
            names.insert("runcommand".to_owned());
        }
    }
    Ok(names)
}

fn normalized_tool_name(name: &str) -> String {
    name.bytes()
        .filter(u8::is_ascii_alphanumeric)
        .map(|byte| char::from(byte.to_ascii_lowercase()))
        .collect()
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
