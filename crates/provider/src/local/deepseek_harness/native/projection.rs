//! One durable DSH event projection shared by live delivery and complete history recovery.
use super::super::{config::text, streaming::Stream};
use crate::{
    ports::agent_session::{AgentSessionError, AgentTurnEvent},
    protocol::timeline::NativeItem,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Match the native agent loop: only an exactly empty argument string means an empty object.
pub(super) fn arguments(raw: &str) -> Value {
    if raw.is_empty() {
        json!({})
    } else {
        serde_json::from_str(raw).unwrap_or_else(|_| json!(raw))
    }
}

/// Whether a native event represents human input rather than injected runtime context.
pub(super) fn is_user_message(event: &Value) -> bool {
    event["type"] == "user/message" && event["data"]["source"]["kind"] == "user"
}

/// Project one ordered native journal event into the supplied stream and tool state.
/// Session identity scopes stable keys. Malformed event envelopes return an error;
/// malformed tool arguments remain native tool input rather than transport failures.
pub(super) fn apply(
    stream: &mut Stream,
    tools: &mut BTreeMap<String, Value>,
    event: &Value,
    session: &str,
) -> Result<(), AgentSessionError> {
    let data = &event["data"];
    let before = stream.events.len();
    match event["type"].as_str() {
        Some("turn/start") => {
            let turn = data["turn"].as_u64().ok_or(AgentSessionError::Failed)?;
            stream.begin(format!("dsh:{session}:{turn}"));
            tools.clear();
        }
        Some("user/message") if is_user_message(event) => stream
            .events
            .push_back(AgentTurnEvent::Timeline(user(event, session)?)),
        Some("assistant/message") => {
            let message = &data["message"];
            for block in message["content"]
                .as_array()
                .ok_or(AgentSessionError::Failed)?
            {
                match block["type"].as_str() {
                    Some("text"|"reasoning") => stream.update(&json!({"sessionUpdate":if block["type"]=="text"{"agent_message_chunk"}else{"agent_thought_chunk"},"messageId":message["id"],"content":{"type":"text","text":block["text"]}}))?,
                    Some("tool-call") => {},
                    _ => return Err(AgentSessionError::Failed),
                }
            }
            stream.flush();
        }
        Some("tool/call") => {
            let id = text(data, "callId")?;
            let input = arguments(
                data["arguments"]
                    .as_str()
                    .ok_or(AgentSessionError::Failed)?,
            );
            if tools.len() >= 4096
                || tools
                    .insert(
                        id.into(),
                        json!({"name":text(data,"name")?,"input":input,"time":event["time"],"seq":event["seq"]}),
                    )
                    .is_some()
            {
                return Err(AgentSessionError::Failed);
            }
            stream.update(&json!({"sessionUpdate":"tool_call","toolCallId":id,"title":data["name"],"status":"in_progress","rawInput":input}))?;
        }
        Some("tool/result") => {
            for block in data["message"]["content"]
                .as_array()
                .ok_or(AgentSessionError::Failed)?
            {
                let id = text(block, "toolCallId")?;
                if !tools.contains_key(id) {
                    return Err(AgentSessionError::Failed);
                }
                stream.update(&json!({"sessionUpdate":"tool_call_update","toolCallId":id,"status":if block["isError"]==true{"failed"}else{"completed"},"rawOutput":block["content"]}))?;
            }
        }
        _ => {}
    }
    // Native identities, rather than transport-local turn UUIDs, survive refresh and restart.
    let mut parts = BTreeMap::new();
    for update in stream.events.iter_mut().skip(before) {
        let (AgentTurnEvent::Progress { entry, .. } | AgentTurnEvent::Timeline(entry)) = update
        else {
            continue;
        };
        if entry.item["type"] == "user_message" {
            continue;
        }
        entry.turn_id = data["turn"]
            .as_u64()
            .map(|turn| format!("dsh:{session}:{turn}"));
        let time = if entry.item["type"] == "tool_call" {
            let id = text(&entry.item, "callId")?;
            let call = tools.get(id).ok_or(AgentSessionError::Failed)?;
            let seq = call["seq"].as_u64().ok_or(AgentSessionError::Failed)?;
            entry.key = format!("native:dsh:v2:{session}:tool:{seq}:{id}");
            &call["time"]
        } else {
            let next = parts.len();
            let part = *parts.entry(entry.key.clone()).or_insert(next);
            entry.key = format!(
                "native:dsh:v2:{session}:event:{}:{part}",
                event["seq"].as_u64().ok_or(AgentSessionError::Failed)?
            );
            if entry.item["type"] == "assistant_message" {
                entry.item["messageId"] = json!(entry.key);
            }
            &event["time"]
        };
        entry.timestamp = timestamp(time)?;
    }
    Ok(())
}

fn timestamp(time: &Value) -> Result<String, AgentSessionError> {
    chrono::DateTime::from_timestamp_millis(time.as_i64().ok_or(AgentSessionError::Failed)?)
        .map(|time| time.to_rfc3339())
        .ok_or(AgentSessionError::Failed)
}

fn user(event: &Value, session: &str) -> Result<NativeItem, AgentSessionError> {
    let message = &event["data"];
    let mut parts = Vec::new();
    for block in message["content"]
        .as_array()
        .ok_or(AgentSessionError::Failed)?
    {
        match block["type"].as_str() {
            Some("text") => parts.push(
                block["text"]
                    .as_str()
                    .ok_or(AgentSessionError::Failed)?
                    .to_owned(),
            ),
            // Image hydration has already materialized images as Markdown text.
            Some("file") => parts.push(format!(
                "[Attachment: {}]",
                block["attachment"]["name"].as_str().unwrap_or("file")
            )),
            _ => return Err(AgentSessionError::Failed),
        }
    }
    let id = text(message, "id")?;
    let mut item = json!({"type":"user_message","text":parts.join("\n"),"messageId":id});
    if let Some(id) = message["source"]["rpcId"].as_str() {
        item["clientMessageId"] = json!(id);
    }
    Ok(NativeItem {
        key: format!("native:dsh:v2:{session}:user:{id}"),
        turn_id: None,
        timestamp: timestamp(&event["time"])?,
        item,
    })
}
