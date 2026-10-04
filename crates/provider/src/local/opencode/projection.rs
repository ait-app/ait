//! Project verified native history into the server's immutable display timeline.
use std::collections::BTreeMap;

use super::types::{Content, Record, Role, Snapshot, ToolStatus};
use crate::{ports::agent_session::AgentSessionError, protocol::timeline::NativeItem};
use serde_json::{Value, json};

/// Return the versioned Ait display key for a native identity.
/// Existing `OpenCode` projections rebuild once; native IDs and other providers are unaffected.
pub(super) fn key(id: &str) -> String {
    format!("native:opencode:projection-v3:{id}")
}

pub(super) fn entries(
    snapshot: &Snapshot,
    clients: &BTreeMap<String, String>,
) -> Result<Vec<NativeItem>, AgentSessionError> {
    records(&snapshot.messages, clients)
}

/// Project normalized native records in their original order, attaching known client IDs.
/// Returns `Failed` for invalid timestamps, tool arguments, or missing tool results.
pub(super) fn records(
    messages: &[Record],
    clients: &BTreeMap<String, String>,
) -> Result<Vec<NativeItem>, AgentSessionError> {
    let results = messages
        .iter()
        .filter_map(|record| {
            record
                .tool_result
                .as_ref()
                .map(|result| (result.call_id.as_str(), result))
        })
        .collect::<BTreeMap<_, _>>();
    let mut entries = Vec::new();
    let mut turn = None;
    for record in messages {
        if record.tool_result.is_some() || record.role == Role::System {
            continue;
        }
        let timestamp = chrono::DateTime::from_timestamp_millis(record.created_at)
            .ok_or(AgentSessionError::Failed)?
            .to_rfc3339();
        if record.role == Role::User {
            turn = Some(record.input_id.clone().unwrap_or_else(|| record.id.clone()));
            let texts = record
                .sub_messages
                .iter()
                .filter_map(|part| match part {
                    Content::Text { text } => Some(text.as_str()),
                    Content::ToolCall(_)
                    | Content::NativeContent(_)
                    | Content::StructuredData { .. } => None,
                })
                .collect::<Vec<_>>();
            if !texts.is_empty() {
                let client = record
                    .input_id
                    .as_ref()
                    .and_then(|id| clients.get(id))
                    .map_or(record.id.as_str(), String::as_str);
                entries.push(NativeItem { key: key(&record.id), turn_id: turn.clone(), timestamp: timestamp.clone(), item: json!({"type":"user_message","messageId":record.id,"clientMessageId":client,"text":texts.join("\n")}) });
            }
            continue;
        }
        for (index, part) in record.sub_messages.iter().enumerate() {
            let key = record.metadata["part_ids"][index]
                .as_str()
                .map_or_else(|| format!("{}:{index}", record.id), str::to_owned);
            let (key, item) = match part {
                Content::Text { text } => (
                    key.clone(),
                    json!({"type":"assistant_message","messageId":key,"text":text}),
                ),
                Content::ToolCall(call) => {
                    let result = results
                        .get(call.call_id.as_str())
                        .ok_or(AgentSessionError::Failed)?;
                    let input: Value = serde_json::from_str(&call.arguments)
                        .map_err(|_| AgentSessionError::Failed)?;
                    let mut detail = detail(&call.tool_name, &input);
                    if let Some(output) = &result.output {
                        let field = if detail["type"] == "read" {
                            "content"
                        } else {
                            "output"
                        };
                        detail[field] = json!(output);
                    }
                    (
                        format!("tool:{}", call.call_id),
                        json!({"type":"tool_call","callId":call.call_id,"name":call.tool_name,
                        "status":if result.status==ToolStatus::Succeeded {"completed"} else {"failed"},"error":result.error,"detail":detail}),
                    )
                }
                Content::NativeContent(content) if content.item_type == "reasoning" => (
                    key,
                    json!({"type":"reasoning","text":content.payload["text"].as_str().unwrap_or_default()}),
                ),
                Content::NativeContent(_) | Content::StructuredData { .. } => continue,
            };
            entries.push(NativeItem {
                key: self::key(&key),
                turn_id: turn.clone(),
                timestamp: timestamp.clone(),
                item,
            });
        }
    }
    Ok(entries)
}

fn detail(name: &str, input: &Value) -> Value {
    match name {
        "bash" | "shell" if input["command"].is_string() => {
            let mut detail = json!({"type":"shell","command":input["command"]});
            if let Some(cwd) = input["cwd"].as_str().or_else(|| input["workdir"].as_str()) {
                detail["cwd"] = json!(cwd);
            }
            detail
        }
        "read" | "edit" | "write" if input["filePath"].is_string() => {
            json!({"type":name,"filePath":input["filePath"]})
        }
        _ => json!({"type":"unknown","input":input,"output":null}),
    }
}

#[cfg(test)]
mod tests;
