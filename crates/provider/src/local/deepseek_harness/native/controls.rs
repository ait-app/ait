//! Structured control results derived from native journal facts, never from English output text.
use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::local::acp_config::text;
use crate::ports::agent_session::AgentSessionError;

#[derive(Debug, Default)]
pub(super) struct State {
    commands: BTreeMap<String, String>,
    goal: Value,
}

impl State {
    /// Seed the current goal from the Host's authoritative opening projection.
    /// Rejects malformed native goal facts without parsing the command's display text.
    pub(super) fn from_projection(value: &Value) -> Result<Self, AgentSessionError> {
        Ok(Self {
            goal: goal(value)?,
            ..Self::default()
        })
    }

    /// Fold one native event and optionally return its independent control display item.
    /// Session identity scopes cards; invalid lifecycle records and exceeded budgets fail.
    pub(super) fn apply(
        &mut self,
        event: &Value,
        session: &str,
    ) -> Result<Option<Value>, AgentSessionError> {
        let data = &event["data"];
        match event["type"].as_str() {
            Some("command/run") => {
                if self.commands.len() >= 128 {
                    return Err(AgentSessionError::Failed);
                }
                self.commands.insert(
                    text(data, "commandId")?.to_owned(),
                    text(data, "name")?.to_owned(),
                );
                Ok(None)
            }
            Some("command/done") => {
                let id = text(data, "commandId")?;
                let name = self
                    .commands
                    .remove(id)
                    .unwrap_or_else(|| "command".to_owned());
                let status = text(data, "kind")?;
                if !matches!(status, "success" | "error") {
                    return Err(AgentSessionError::Failed);
                }
                let mut item = json!({"type":"provider_command",
                    "id":format!("dsh:{session}:command:{id}"),"command":name,
                    "status":status,"text":data["text"].as_str().unwrap_or_default()});
                if name == "goal" {
                    self.decorate_goal(&mut item);
                }
                Ok(Some(item))
            }
            Some("goal/change") => {
                if data["version"] != 1 {
                    return Err(AgentSessionError::Failed);
                }
                self.goal = if data["operation"] == "clear" {
                    Value::Null
                } else {
                    goal(data)?
                };
                if self.commands.values().any(|name| name == "goal") {
                    return Ok(None);
                }
                let seq = event["seq"].as_u64().ok_or(AgentSessionError::Failed)?;
                let mut item = json!({"type":"provider_command","id":format!("dsh:{session}:goal:{seq}"),
                    "command":"goal","status":"success","text":""});
                self.decorate_goal(&mut item);
                Ok(Some(item))
            }
            Some("user/message") if data["source"]["kind"] == "goal" => {
                if self.goal["id"] == data["source"]["goalId"] && self.goal.is_object() {
                    let round = data["source"]["round"]
                        .as_u64()
                        .ok_or(AgentSessionError::Failed)?;
                    self.goal["rounds"] =
                        json!(round.max(self.goal["rounds"].as_u64().unwrap_or_default()));
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn decorate_goal(&self, item: &mut Value) {
        item["goal"] = self.goal.clone();
        let phase = self.goal["phase"].as_str();
        let actions: &[&str] = match phase {
            Some("active") => &["pause", "edit", "clear"],
            Some("paused" | "blocked") => &["resume", "edit", "clear"],
            Some("complete") => &["edit", "clear"],
            _ => &[],
        };
        item["actions"] = json!(
            actions
                .iter()
                .map(|action| json!({"id":action,
            "command":format!("/goal {action}{}",if *action == "edit" {" "} else {""})}))
                .collect::<Vec<_>>()
        );
    }
}

fn goal(value: &Value) -> Result<Value, AgentSessionError> {
    if value.is_null() {
        return Ok(Value::Null);
    }
    let native = &value["goal"];
    let phase = text(native, "phase")?;
    if !matches!(phase, "active" | "paused" | "blocked" | "complete") {
        return Err(AgentSessionError::Failed);
    }
    let revision = native["revision"]
        .as_u64()
        .filter(|value| *value > 0)
        .ok_or(AgentSessionError::Failed)?;
    let limit = native["maxGoalRounds"]
        .as_u64()
        .filter(|value| *value > 0)
        .ok_or(AgentSessionError::Failed)?;
    let rounds = value["roundsStarted"]
        .as_u64()
        .ok_or(AgentSessionError::Failed)?;
    let mut projected = json!({"id":text(native,"id")?,"revision":revision,
        "phase":phase,"objective":text(native,"objective")?,"rounds":rounds,"maxRounds":limit});
    if phase == "blocked" {
        projected["blockedReason"] = json!(text(&native["blockedReason"], "message")?);
    }
    Ok(projected)
}

#[cfg(test)]
mod tests;
