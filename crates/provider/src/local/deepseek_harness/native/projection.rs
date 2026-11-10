//! One durable DSH event projection shared by live delivery and complete history recovery.
use crate::local::acp_config::text;
use crate::local::acp_streaming::Stream;
use crate::{
    ports::agent_session::{AgentSessionError, AgentTurnEvent},
    protocol::timeline::NativeItem,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Debug, Default)]
pub(super) struct State {
    pub(super) tools: BTreeMap<String, Value>,
    controls: super::controls::State,
}

impl State {
    /// Project current interactive controls independently of timeline pagination.
    pub(super) fn ui_state(&self, session: &str) -> Option<Value> {
        let goal = self.controls.current_goal(session)?;
        Some(json!({"goal":goal,"statuses":[],"widgets":[],"title":null,"editor":null,"link":null}))
    }

    /// Initialize control facts from the native opening projection, rejecting malformed goals.
    pub(super) fn from_projection(values: &Value) -> Result<Self, AgentSessionError> {
        Ok(Self {
            controls: super::controls::State::from_projection(&values["goal"])?,
            ..Self::default()
        })
    }
}

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
    state: &mut State,
    event: &Value,
    session: &str,
) -> Result<(), AgentSessionError> {
    let data = &event["data"];
    let before = stream.events.len();
    apply_controls(stream, state, event, session)?;
    let tools = &mut state.tools;
    match event["type"].as_str() {
        Some("turn/start") => {
            let turn = data["turn"].as_u64().ok_or(AgentSessionError::Failed)?;
            stream.begin(format!("dsh:{session}:{turn}"));
            tools.clear();
        }
        Some("turn/end") => stream.finish_compactions(),
        Some("user/message") if is_user_message(event) => stream
            .events
            .push_back(AgentTurnEvent::Timeline(user(event, session)?)),
        Some("command/run") if data["source"]["kind"] == "user" => stream
            .events
            .push_back(AgentTurnEvent::Timeline(command_input(event, session)?)),
        Some("compaction/start" | "compaction/end") => compaction(stream, event, session)?,
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
            let message = &data["message"];
            let content = message["content"]
                .as_array()
                .ok_or(AgentSessionError::Failed)?;
            if message.get("toolCallId").is_some() {
                tool_result(stream, tools, message)?;
            } else {
                // Older Hosts wrapped each result in a content block.
                for block in content {
                    tool_result(stream, tools, block)?;
                }
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

fn apply_controls(
    stream: &mut Stream,
    state: &mut State,
    event: &Value,
    session: &str,
) -> Result<(), AgentSessionError> {
    if let Some(item) = state.controls.apply(event, session)? {
        stream
            .events
            .push_back(AgentTurnEvent::Timeline(NativeItem {
                key: String::new(),
                turn_id: None,
                timestamp: String::new(),
                item,
            }));
    }
    Ok(())
}

fn command_input(event: &Value, session: &str) -> Result<NativeItem, AgentSessionError> {
    let data = &event["data"];
    let id = text(data, "commandId")?;
    let name = text(data, "name")?;
    // Omitted args are intentional for commands with recordInput=false. Do not reconstruct
    // their potentially private input from a client draft or another domain event.
    let args = match data.get("args") {
        Some(Value::String(args)) => args.as_str(),
        None => "",
        Some(_) => return Err(AgentSessionError::Failed),
    };
    Ok(NativeItem {
        key: format!("native:dsh:v2:{session}:command:{id}"),
        turn_id: None,
        timestamp: timestamp(&event["time"])?,
        item: json!({"type":"user_message","messageId":format!("command:{id}"),
            "text":format!("/{name}{args}")}),
    })
}

fn compaction(stream: &mut Stream, event: &Value, session: &str) -> Result<(), AgentSessionError> {
    let data = &event["data"];
    let started = event["type"] == "compaction/start";
    let id = match data.get("compactionId") {
        Some(_) => text(data, "compactionId")?.to_owned(),
        None if !started && stream.native_compaction.is_some() => stream
            .native_compaction
            .clone()
            .expect("opening compaction identity"),
        None => format!(
            "dsh:{session}:{}",
            event["seq"].as_u64().ok_or(AgentSessionError::Failed)?
        ),
    };
    stream.native_compaction = started.then(|| id.clone());
    let status = if event["type"] == "compaction/start" {
        "loading"
    } else if data.get("error").is_some_and(|error| !error.is_null()) {
        "failed"
    } else {
        "completed"
    };
    let trigger = match data.get("turn") {
        Some(Value::Null) => "manual",
        Some(turn) if turn.as_u64().is_some() => "auto",
        _ => return Err(AgentSessionError::Failed),
    };
    stream.compaction(&id, status, Some(trigger))
}

fn tool_result(
    stream: &mut Stream,
    tools: &BTreeMap<String, Value>,
    result: &Value,
) -> Result<(), AgentSessionError> {
    let id = text(result, "toolCallId")?;
    if !tools.contains_key(id) {
        return Err(AgentSessionError::Failed);
    }
    stream.update(&json!({"sessionUpdate":"tool_call_update","toolCallId":id,
        "status":if result["isError"]==true{"failed"}else{"completed"},
        "rawOutput":result["content"]}))
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

#[cfg(test)]
mod tests;
