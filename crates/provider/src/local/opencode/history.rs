//! Version-specific records become transport-free content, without assigning Ait identities.
use std::collections::HashSet;

use crate::local::opencode::types::Record;
use crate::local::opencode::types::{
    Content, Fault, NativeContent, ProtocolError, Role, ToolCall, ToolOutput, ToolStatus,
};
use serde_json::{Value, json};

use super::{
    failure,
    http::{Version, required_string},
};

pub(super) fn normalize(
    version: Version,
    session: &str,
    messages: &[Value],
) -> Result<Vec<Record>, ProtocolError> {
    normalize_messages(version, session, messages, false)
}

fn normalize_messages(
    version: Version,
    session: &str,
    messages: &[Value],
    partial_last: bool,
) -> Result<Vec<Record>, ProtocolError> {
    let mut output = Vec::with_capacity(messages.len());
    let mut ids = HashSet::with_capacity(messages.len());
    let mut calls = HashSet::new();
    for (index, message) in messages.iter().enumerate() {
        let info = match version {
            Version::V1 => message.get("info").unwrap_or(&Value::Null),
            Version::V2 => message,
        };
        let id = required_string(info, "id")?;
        if !ids.insert(id.to_owned())
            || (version == Version::V1
                && info.get("sessionID").and_then(Value::as_str) != Some(session))
        {
            return Err(failure(
                Fault::ProviderFailed,
                "OpenCode history identity mismatch",
            ));
        }
        let mut record = new_record(
            version,
            session,
            info,
            partial_last && index + 1 == messages.len(),
        )?;
        let kind = required_string(
            info,
            if version == Version::V1 {
                "role"
            } else {
                "type"
            },
        )?;
        let role = record.role;
        if let Some(text) = info.get("text").and_then(Value::as_str) {
            record.sub_messages.push(Content::Text {
                text: text.to_owned(),
            });
        }
        let parts = match version {
            Version::V1 => message.get("parts").and_then(Value::as_array),
            Version::V2 => info.get("content").and_then(Value::as_array),
        };
        let mut results = Vec::new();
        if version == Version::V1 {
            record.metadata["part_ids"] = json!(
                parts
                    .into_iter()
                    .flatten()
                    .map(|part| part["id"].clone())
                    .collect::<Vec<_>>()
            );
        }
        let mut part_ids = HashSet::new();
        if let Some(parts) = parts {
            for (ordinal, part) in parts.iter().enumerate() {
                if version == Version::V1
                    && (part.get("sessionID").and_then(Value::as_str) != Some(session)
                        || part.get("messageID").and_then(Value::as_str) != Some(id)
                        || !part_ids.insert(required_string(part, "id")?.to_owned()))
                {
                    return Err(failure(
                        Fault::ProviderFailed,
                        "OpenCode history part identity mismatch",
                    ));
                }
                normalize_part(
                    version,
                    &mut record,
                    (ordinal, part),
                    &mut results,
                    &mut calls,
                )?;
            }
        } else if version == Version::V1 || kind == "assistant" {
            return Err(failure(
                Fault::ProviderFailed,
                "OpenCode history omitted message content",
            ));
        }
        if record.sub_messages.is_empty() {
            record.sub_messages.push(native_item(id, kind, 0, info));
        }
        if role != Role::Assistant {
            for part in &mut record.sub_messages {
                if let Content::NativeContent(item) = part {
                    *part = Content::StructuredData {
                        media_type: "application/vnd.opencode+json".into(),
                        value: item.payload.to_string(),
                    };
                }
            }
        }
        output.push(record);
        output.extend(results);
    }
    Ok(output)
}

fn normalize_part(
    version: Version,
    record: &mut Record,
    indexed_part: (usize, &Value),
    results: &mut Vec<Record>,
    calls: &mut HashSet<String>,
) -> Result<(), ProtocolError> {
    let (ordinal, part) = indexed_part;
    let kind = required_string(part, "type")?;
    match kind {
        "text" => record.sub_messages.push(Content::Text {
            text: part
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| failure(Fault::ProviderFailed, "OpenCode text content missing"))?
                .to_owned(),
        }),
        "tool" => {
            if record.role != Role::Assistant {
                return Err(failure(
                    Fault::ToolUseRequiresAssistant,
                    "OpenCode tool appeared in a non-assistant message",
                ));
            }
            let call_id = required_string(
                part,
                match version {
                    Version::V1 => "callID",
                    Version::V2 => "id",
                },
            )?;
            if !calls.insert(call_id.to_owned()) {
                return Err(failure(
                    Fault::ToolCallDuplicate,
                    "duplicate OpenCode tool call identity",
                ));
            }
            let tool = required_string(
                part,
                match version {
                    Version::V1 => "tool",
                    Version::V2 => "name",
                },
            )?;
            let state = part
                .get("state")
                .ok_or_else(|| failure(Fault::ProviderFailed, "OpenCode tool state missing"))?;
            let status = match required_string(state, "status")? {
                "completed" => ToolStatus::Succeeded,
                "error" => ToolStatus::Failed,
                "pending" | "streaming" | "running" => {
                    return Err(failure(
                        Fault::RunRecoveryFailed,
                        "OpenCode history contains an unfinished tool",
                    ));
                }
                _ => {
                    return Err(failure(
                        Fault::AgentCapabilityUnsupported,
                        "unknown OpenCode tool status",
                    ));
                }
            };
            let input = state
                .get("input")
                .filter(|input| input.is_object())
                .ok_or_else(|| failure(Fault::ProviderFailed, "invalid OpenCode tool input"))?;
            record.sub_messages.push(Content::ToolCall(ToolCall {
                call_id: call_id.to_owned(),
                tool_name: tool.to_owned(),
                arguments: safe_value(input, 0).to_string(),
                provider_metadata: None,
            }));
            let output = match version {
                Version::V1 => state
                    .get("output")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                Version::V2 => state
                    .get("content")
                    .map(|content| safe_value(content, 0).to_string()),
            };
            results.push(Record {
                id: format!("{}#result:{call_id}", record.id),
                role: Role::User,
                sub_messages: Vec::new(),
                tool_result: Some(ToolOutput {
                    call_id: call_id.to_owned(),
                    status,
                    output,
                    error: (status == ToolStatus::Failed)
                        .then(|| "OpenCode tool failed".to_owned()),
                }),
                input_id: None,
                created_at: record.created_at,
                metadata: record.metadata.clone(),
            });
        }
        _ => record
            .sub_messages
            .push(native_item(&record.id, kind, ordinal, part)),
    }
    Ok(())
}

fn native_item(id: &str, kind: &str, ordinal: usize, value: &Value) -> Content {
    Content::NativeContent(NativeContent {
        provider_kind: "opencode".into(),
        external_item_id: format!("{id}:{ordinal}"),
        item_type: kind.to_owned(),
        ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
        payload: safe_value(value, 0),
        payload_schema_version: 1,
    })
}

fn safe_value(value: &Value, depth: usize) -> Value {
    if depth >= 16 {
        return Value::Null;
    }
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .take(256)
                .map(|(key, value)| {
                    let sensitive = matches!(
                        key.to_ascii_lowercase().as_str(),
                        "authorization"
                            | "password"
                            | "token"
                            | "api_key"
                            | "apikey"
                            | "secret"
                            | "environment"
                            | "env"
                            | "headers"
                            | "providerstate"
                            | "providerresultstate"
                    );
                    (
                        key.clone(),
                        if sensitive {
                            Value::String("[REDACTED]".into())
                        } else {
                            safe_value(value, depth + 1)
                        },
                    )
                })
                .collect(),
        ),
        Value::Array(array) => Value::Array(
            array
                .iter()
                .take(256)
                .map(|value| safe_value(value, depth + 1))
                .collect(),
        ),
        Value::String(text) => Value::String(text[..text.floor_char_boundary(262_144)].to_owned()),
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
    }
}

fn new_record(
    version: Version,
    session: &str,
    info: &Value,
    partial: bool,
) -> Result<Record, ProtocolError> {
    let id = required_string(info, "id")?;
    let kind = required_string(
        info,
        match version {
            Version::V1 => "role",
            Version::V2 => "type",
        },
    )?;
    let role = match kind {
        "user" => Role::User,
        "system" => Role::System,
        "assistant" | "synthetic" | "skill" | "shell" | "compaction" | "idle"
        | "agent-switched" | "model-switched" | "location-switched" => Role::Assistant,
        _ => {
            return Err(failure(
                Fault::AgentCapabilityUnsupported,
                "unsupported OpenCode history message type",
            ));
        }
    };
    if kind == "assistant"
        && !partial
        && info
            .pointer("/time/completed")
            .and_then(Value::as_i64)
            .is_none()
    {
        return Err(failure(
            Fault::RunRecoveryFailed,
            "OpenCode history contains an unfinished assistant",
        ));
    }
    let created_at = info
        .pointer("/time/created")
        .and_then(Value::as_i64)
        .ok_or_else(|| failure(Fault::ProviderFailed, "invalid OpenCode message timestamp"))?;
    Ok(Record {
        id: id.to_owned(),
        role,
        sub_messages: Vec::new(),
        tool_result: None,
        input_id: if kind == "user" {
            match version {
                Version::V1 => Some(id.to_owned()),
                Version::V2 => info
                    .pointer("/metadata/aitInputId")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            }
        } else {
            None
        },
        created_at,
        metadata: json!({"native_message_id": id, "native_session_id": session,
                "protocol_version": if version == Version::V1 {1} else {2}, "native_type":kind}),
    })
}

mod prefix;
pub(super) use prefix::before_text;
