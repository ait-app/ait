use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::ports::agent_session::{AgentSessionError, AgentTurnEvent};
use crate::protocol::prompt::{AgentPrompt, PromptImage};

/// Deduplicate standard ACP text and tools while preserving the native host event channel.
#[derive(Debug, Default)]
pub(super) struct Projection {
    text: BTreeMap<String, usize>,
    tools: BTreeSet<String>,
}

impl Projection {
    /// Project `event` for `session`; returns bounded standard updates or invalid display facts.
    pub(super) fn update(
        &mut self,
        session: &str,
        event: &AgentTurnEvent,
    ) -> Result<Vec<Value>, AgentSessionError> {
        let (entry, incremental) = match event {
            AgentTurnEvent::Progress { entry, .. } => (entry, true),
            AgentTurnEvent::Timeline(entry) => (entry, false),
            AgentTurnEvent::Usage(usage) => {
                return Ok(usage.context_window_used_tokens.zip(usage.context_window_max_tokens).map(|(used,size)|{
                    let mut update=json!({"sessionUpdate":"usage_update","used":used,"size":size});
                    if let Some(amount)=usage.total_cost_usd {update["cost"]=json!({"amount":amount,"currency":"USD"});}
                    json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":session,
                        "update":update}})
                }).into_iter().collect());
            }
            _ => return Ok(Vec::new()),
        };
        if self.text.len() + self.tools.len() >= 4096
            && !self.text.contains_key(&entry.key)
            && !self.tools.contains(&entry.key)
        {
            return Err(AgentSessionError::Failed);
        }
        let item = &entry.item;
        let update = match item["type"].as_str() {
            Some("assistant_message" | "reasoning" | "user_message") => {
                let text = item["text"].as_str().ok_or(AgentSessionError::Failed)?;
                let seen = self.text.entry(entry.key.clone()).or_default();
                let delta = if incremental {
                    text
                } else {
                    text.get(*seen..).unwrap_or(text)
                };
                if delta.is_empty() {
                    if !incremental {
                        self.text.remove(&entry.key);
                    }
                    return Ok(Vec::new());
                }
                *seen = if incremental {
                    seen.saturating_add(text.len())
                } else {
                    text.len()
                };
                json!({"sessionUpdate":match item["type"].as_str() {Some("reasoning")=>"agent_thought_chunk",Some("user_message")=>"user_message_chunk",_=>"agent_message_chunk"},
                    "messageId":entry.key,"content":{"type":"text","text":delta}})
            }
            Some("tool_call") => {
                let first = self.tools.insert(entry.key.clone());
                let status = match item["status"].as_str() {
                    Some("running") => "in_progress",
                    Some("completed") => "completed",
                    _ => "failed",
                };
                json!({"sessionUpdate":if first {"tool_call"} else {"tool_call_update"},
                    "toolCallId":item["callId"],"title":item["name"],"kind":"other",
                    "status":status,
                    "rawInput":item["detail"]["input"],"rawOutput":item["detail"]["output"]})
            }
            Some("notification") => {
                let message = item["message"].as_str().ok_or(AgentSessionError::Failed)?;
                json!({"sessionUpdate":"tool_call","toolCallId":entry.key,
                    "title":"Provider notification","kind":"other","status":"completed",
                    "rawOutput":message,"_meta":{"level":item["level"]}})
            }
            _ => return Ok(Vec::new()),
        };
        if !incremental {
            self.text.remove(&entry.key);
            self.tools.remove(&entry.key);
        }
        Ok(vec![
            json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":session,"update":update}}),
        ])
    }
}

/// Decode standard content blocks, or validated lossless Ait metadata when `ait` is negotiated.
/// Returns invalid/unsupported inputs before native submission.
pub(super) fn prompt(params: &Value, ait: bool) -> Result<AgentPrompt, AgentSessionError> {
    if ait && !params["_meta"]["ait"]["prompt"].is_null() {
        let prompt: AgentPrompt = serde_json::from_value(params["_meta"]["ait"]["prompt"].clone())
            .map_err(|_| AgentSessionError::Rejected)?;
        if params["prompt"].as_array() != Some(&prompt.blocks()?) {
            return Err(AgentSessionError::Rejected);
        }
        return Ok(prompt);
    }
    let blocks = params["prompt"]
        .as_array()
        .filter(|blocks| blocks.len() <= 128)
        .ok_or(AgentSessionError::Rejected)?;
    let mut prompt = AgentPrompt::default();
    for block in blocks {
        match block["type"].as_str() {
            Some("text") => {
                if !prompt.text.is_empty() {
                    prompt.text.push_str("\n\n");
                }
                prompt
                    .text
                    .push_str(block["text"].as_str().ok_or(AgentSessionError::Rejected)?);
            }
            Some("resource") if block["resource"]["text"].is_string() => {
                if !prompt.text.is_empty() {
                    prompt.text.push_str("\n\n");
                }
                prompt.text.push_str(
                    block["resource"]["text"]
                        .as_str()
                        .ok_or(AgentSessionError::Rejected)?,
                );
            }
            Some("image") => prompt.images.push(PromptImage {
                data: block["data"]
                    .as_str()
                    .ok_or(AgentSessionError::Rejected)?
                    .to_owned(),
                mime_type: block["mimeType"]
                    .as_str()
                    .ok_or(AgentSessionError::Rejected)?
                    .to_owned(),
            }),
            _ => return Err(AgentSessionError::Rejected),
        }
    }
    prompt.validate()?;
    Ok(prompt)
}

/// Build a standard permission/form callback for `request` in `session`.
/// Returns no callback when form support is absent, or invalid native question facts.
pub(super) fn question(
    session: &str,
    request: &Value,
    elicitation: bool,
) -> Result<Option<Value>, AgentSessionError> {
    let id = request["id"].as_str().ok_or(AgentSessionError::Failed)?;
    if request["kind"] == "tool" {
        return Ok(Some(
            json!({"jsonrpc":"2.0","id":format!("ui-{id}"),"method":"session/request_permission",
            "params":{"sessionId":session,"toolCall":{"toolCallId":id,"title":request["description"],"status":"pending"},
                "options":[{"optionId":"allow","name":"Allow once","kind":"allow_once"},
                    {"optionId":"deny","name":"Reject","kind":"reject_once"}]}}),
        ));
    }
    if !elicitation {
        return Ok(None);
    }
    let questions = request["input"]["questions"]
        .as_array()
        .ok_or(AgentSessionError::Failed)?;
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for question in questions {
        let key = question["answerKey"]
            .as_str()
            .ok_or(AgentSessionError::Failed)?;
        let labels = question["options"]
            .as_array()
            .ok_or(AgentSessionError::Failed)?
            .iter()
            .map(|option| option["label"].clone())
            .collect::<Vec<_>>();
        let mut choice = json!({"type":"string"});
        if question["allowOther"] != true && !labels.is_empty() {
            choice["enum"] = json!(labels);
        }
        let array = question["answerFormat"] == "array";
        let mut field = if array {
            json!({"type":"array","items":choice,"uniqueItems":true,
            "minItems":1,"maxItems":if question["multiSelect"]==true {128} else {1}})
        } else {
            choice
        };
        field["title"] = question["question"].clone();
        properties.insert(key.to_owned(), field);
        required.push(key);
    }
    Ok(Some(
        json!({"jsonrpc":"2.0","id":format!("ui-{id}"),"method":"elicitation/create",
        "params":{"mode":"form","message":request["description"].as_str().unwrap_or("OMP extension question"),
            "requestedSchema":{"type":"object","properties":properties,"required":required}}}),
    ))
}

/// Convert an ACP callback `result` into the normalized `request` answer.
/// Rejects invalid selections/actions; omitted or cancelled results revoke permission.
pub(super) fn answer(request: &Value, result: &Value) -> Result<Value, AgentSessionError> {
    if request["kind"] == "tool" {
        return match result["outcome"]["outcome"].as_str() {
            Some("selected") => match result["outcome"]["optionId"].as_str() {
                Some("allow") => Ok(json!({"behavior":"allow"})),
                Some("deny") => Ok(json!({"behavior":"deny"})),
                _ => Err(AgentSessionError::Rejected),
            },
            Some("cancelled") | None => Ok(json!({"behavior":"deny"})),
            _ => Err(AgentSessionError::Rejected),
        };
    }
    match result["action"].as_str() {
        Some("accept") => {
            Ok(json!({"behavior":"allow","updatedInput":{"answers":result["content"]}}))
        }
        Some("cancel" | "decline") | None => Ok(json!({"behavior":"deny"})),
        _ => Err(AgentSessionError::Rejected),
    }
}
