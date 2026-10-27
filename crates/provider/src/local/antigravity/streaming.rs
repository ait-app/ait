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
    reported_denials: BTreeMap<(String, String), usize>,
    compactions: crate::local::compaction::Compactions,
    pub(super) failure: Option<Failure>,
    pub(super) explanation: crate::local::diagnostics::failure::Failure,
}

impl Stream {
    pub(super) fn reset_process(&mut self) {
        self.reported_denials.clear();
    }

    pub(super) fn begin(&mut self, turn: String, conversation: String) {
        self.steps.clear();
        self.turn = turn;
        self.conversation = conversation;
        self.observation = 0;
        self.bytes = 0;
        self.assistant_seen = false;
        self.compactions = crate::local::compaction::Compactions::default();
        self.failure = None;
        self.explanation.clear();
    }

    pub(super) fn update(&mut self, update: &Value) -> Result<(), AgentSessionError> {
        if update["conversation_id"] != self.conversation {
            return Err(AgentSessionError::Failed);
        }
        let done = match update["state"].as_str() {
            Some("ACTIVE") => false,
            Some("DONE" | "ERROR") => true,
            _ => return Err(AgentSessionError::Failed),
        };
        let kind = match update["step_type"].as_str() {
            Some("agent_response") => "assistant_message",
            Some("tool" | "subagent") => "tool_call",
            Some("checkpoint") => return self.compaction(update, done),
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
            if update["step_type"] == "tool"
                && update["state"] == "DONE"
                && step.tool["output"].is_null()
                && step.tool["error"].is_null()
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

    fn compaction(&mut self, update: &Value, done: bool) -> Result<(), AgentSessionError> {
        let index = update["step_index"]
            .as_u64()
            .ok_or(AgentSessionError::Failed)?;
        let id = format!("{}:{}:checkpoint:{index}", self.conversation, self.turn);
        let status = if update["state"] == "ERROR" {
            "failed"
        } else if done {
            "completed"
        } else {
            "loading"
        };
        if let Some(item) = self.compactions.marker(&id, status, Some("auto"))? {
            self.events.push_back(AgentTurnEvent::Timeline(NativeItem {
                key: format!("native:antigravity:{id}:{status}"),
                turn_id: Some(self.turn.clone()),
                timestamp: Utc::now().to_rfc3339(),
                item,
            }));
        }
        Ok(())
    }

    pub(super) fn finish(&mut self, result: &Value) -> Result<(), AgentSessionError> {
        if result["conversation_id"] != self.conversation {
            return Err(AgentSessionError::Failed);
        }
        let status = result["status"].as_str().ok_or(AgentSessionError::Failed)?;
        let denied_tools = denied_tools(result, &mut self.reported_denials)?;
        let denied = !denied_tools.is_empty();
        let canceled = matches!(status, "CANCELED" | "INTERRUPTED")
            || (status == "ERROR" && result["error"] == "interrupted");
        let response = result["response"]
            .as_str()
            .ok_or(AgentSessionError::Failed)?;
        if response.len() > MAX_TEXT {
            return Err(AgentSessionError::Failed);
        }
        self.finish_compactions();
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
        let failed = !canceled && (status != "SUCCESS" || (denied && response.trim().is_empty()));
        if failed {
            if !denied && !result["error"].is_null() {
                self.explanation.observe(
                    "Antigravity",
                    &result["error"],
                    crate::local::diagnostics::native(&result["error"], "result"),
                );
            }
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
                    if !refused && canceled {
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
            _ if canceled => AgentTurnEvent::Cancelled,
            "SUCCESS" if failed => AgentTurnEvent::Failed,
            "SUCCESS" => AgentTurnEvent::Completed(Some(response.to_owned())),
            _ => AgentTurnEvent::Failed,
        });
        Ok(())
    }

    pub(super) fn abort(&mut self, failure: Failure) {
        self.finish_compactions();
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

    fn finish_compactions(&mut self) {
        for item in self.compactions.fail_pending() {
            let id = item["compactionId"]
                .as_str()
                .expect("native compaction identity");
            self.events.push_back(AgentTurnEvent::Timeline(NativeItem {
                key: format!("native:antigravity:{id}:failed"),
                turn_id: Some(self.turn.clone()),
                timestamp: Utc::now().to_rfc3339(),
                item,
            }));
        }
    }
}

fn update_tool(step: &mut Step, update: &Value, done: bool) -> Result<usize, AgentSessionError> {
    let mut bytes = 0_usize;
    if let Some(info) = update.get("tool_info").filter(|info| !info.is_null()) {
        let fields = info.as_object().ok_or(AgentSessionError::Failed)?;
        for (name, value) in fields {
            let (value, size) = preview(value);
            bytes = bytes.saturating_add(size);
            step.tool[name] = value;
        }
    }
    if let Some(name) = update.get("tool_name").filter(|name| *name != "") {
        step.tool["name"] = name.clone();
    }
    let name = step.tool["name"]
        .as_str()
        .filter(|name| !name.is_empty() && name.len() <= 512 && !name.chars().any(char::is_control))
        .ok_or(AgentSessionError::Failed)?;
    let failed = done && (update["state"] == "ERROR" || !step.tool["error"].is_null());
    let error = if !failed {
        Value::Null
    } else if step.tool["error"].is_null() {
        json!({"message":"Antigravity tool failed"})
    } else {
        step.tool["error"].clone()
    };
    step.entry.item = json!({"type":"tool_call","callId":step.entry.key,"name":name,
        "status":if failed { "failed" } else if done { "completed" } else { "running" },
        "detail":tool_detail(&step.tool),"error":error});
    if let Some(info) = update.get("subagent_info").filter(|info| !info.is_null()) {
        let (info, size) = preview(info);
        bytes = bytes.saturating_add(size);
        if let Some(child) = info["subagents"]
            .as_array()
            .and_then(|children| children.first())
        {
            let mut detail = json!({"type":"sub_agent","log":child["log_uri"].as_str().unwrap_or_default(),"actions":[]});
            for (source, target) in [
                ("type_name", "subAgentType"),
                ("initial_prompt", "description"),
                ("conversation_id", "childSessionId"),
            ] {
                if let Some(value) = child[source].as_str() {
                    detail[target] = json!(value);
                }
            }
            step.entry.item["detail"] = detail;
        }
        step.entry.item["metadata"] = json!({"subagentInfo":info});
    }
    Ok(bytes)
}

fn denied_tools(
    result: &Value,
    reported: &mut BTreeMap<(String, String), usize>,
) -> Result<BTreeSet<String>, AgentSessionError> {
    let mut names = BTreeSet::new();
    let mut counts = BTreeMap::new();
    let Some(actions) = result.get("denied_actions") else {
        return Ok(names);
    };
    let actions = actions.as_array().ok_or(AgentSessionError::Failed)?;
    if actions.len() > MAX_ITEMS {
        return Err(AgentSessionError::Failed);
    }
    for action in actions {
        let kind = action["action"].as_str().ok_or(AgentSessionError::Failed)?;
        let display = action["display_name"]
            .as_str()
            .ok_or(AgentSessionError::Failed)?;
        if display.is_empty()
            || display.len() > 512
            || display.chars().any(char::is_control)
            || kind.is_empty()
            || kind.len() > 512
            || kind.chars().any(char::is_control)
        {
            return Err(AgentSessionError::Failed);
        }
        let key = (kind.to_owned(), display.to_owned());
        let count = counts.entry(key.clone()).or_insert(0);
        *count += 1;
        if *count > reported.get(&key).copied().unwrap_or_default() {
            names.insert(normalized_tool_name(display));
            if kind == "command" {
                names.insert("runcommand".to_owned());
            }
        }
    }
    // Results repeat all denials since process startup, not just the current turn.
    for (key, count) in counts {
        let previous = reported.entry(key).or_default();
        *previous = (*previous).max(count);
    }
    if reported.len() > MAX_ITEMS {
        return Err(AgentSessionError::Failed);
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
    let path = parameters["AbsolutePath"]
        .as_str()
        .or_else(|| parameters["TargetFile"].as_str());
    match tool["name"].as_str() {
        Some("run_command")
            if parameters["CommandLine"].is_string()
                && (tool["output"].is_null() || tool["output"].is_string()) =>
        {
            let mut detail = json!({"type":"shell","command":parameters["CommandLine"]});
            if let Some(output) = tool["output"].as_str() {
                detail["output"] = json!(output);
            }
            if let Some(cwd) = parameters["Cwd"].as_str() {
                detail["cwd"] = json!(cwd);
            }
            detail
        }
        Some("view_file")
            if path.is_some() && (tool["output"].is_null() || tool["output"].is_string()) =>
        {
            let mut detail = json!({"type":"read","filePath":path});
            if let Some(content) = tool["output"].as_str() {
                detail["content"] = json!(content);
            }
            detail
        }
        Some("write_to_file") if path.is_some() => json!({"type":"write","filePath":path}),
        Some("replace_file_content" | "multi_replace_file_content") if path.is_some() => {
            json!({"type":"edit","filePath":path})
        }
        _ => json!({"type":"unknown","input":parameters,"output":tool["output"]}),
    }
}

#[cfg(test)]
mod tests;
