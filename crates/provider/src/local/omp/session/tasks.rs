//! Route task input through public native lifecycle interfaces, separate from GUI configuration.

use serde_json::{Value, json};

use super::{AgentPrompt, AgentSessionError, AgentTurnEvent, Session, TurnPhase, Uuid};
use crate::local::omp::{commands, vibe::host::Host};

/// A task may substitute native command text, or adopt the native continuation already started.
#[derive(Default)]
pub(super) struct Prepared {
    pub(super) prompt: Option<AgentPrompt>,
    pub(super) native_turn: Option<String>,
}

/// Only objective-bearing goal forms need foreground admission; lifecycle controls do not infer.
pub(super) fn starts_goal(text: &str) -> bool {
    let Some((name, args)) = commands::split(text) else {
        return false;
    };
    name == "guided-goal"
        || (name == "goal"
            && !args.is_empty()
            && !matches!(
                args.split_whitespace().next(),
                Some("show" | "pause" | "resume" | "drop" | "budget")
            ))
}

impl Session {
    /// Effective project commands retain their own semantics, including goal-shaped names.
    pub(super) fn owns_goal(&self, text: &str) -> bool {
        self.vibe_host.is_some()
            && ["goal", "guided-goal"]
                .iter()
                .any(|name| commands::owns_control(text, &self.native_commands, name))
    }

    /// Create native state before foreground work, without submitting a duplicate continuation.
    pub(super) async fn prepare_goal(
        &mut self,
        prompt: &AgentPrompt,
    ) -> Result<Prepared, AgentSessionError> {
        if !self.owns_goal(&prompt.text) || !starts_goal(&prompt.text) {
            return Ok(Prepared::default());
        }
        if self.vibe_host.as_ref().is_some_and(Host::enabled) {
            return Err(AgentSessionError::Busy);
        }
        let (name, args) = commands::split(&prompt.text).ok_or(AgentSessionError::Rejected)?;
        let before = self.transport.request("goal", json!({"op":"get"})).await?;
        if before["state"].is_object() {
            return Err(AgentSessionError::Busy);
        }
        let mut input = prompt.clone();
        if name == "guided-goal" {
            input.text = format!("/ait-guided-goal-v1 {args}");
            return Ok(Prepared {
                prompt: Some(input),
                native_turn: None,
            });
        }
        let objective = args.strip_prefix("set ").unwrap_or(args).trim();
        if objective.is_empty() {
            return Err(AgentSessionError::Rejected);
        }
        let events = self.transport.local_command("/ait-goal-tools-v1").await?;
        self.transport.restore_events(events)?;
        let result = self
            .transport
            .request("goal", json!({"op":"create","objective":objective}))
            .await?;
        self.confirm_goal(prompt, &result);
        let state = self.transport.request("get_state", json!({})).await?;
        if state["sessionId"] != self.state["sessionId"] {
            return Err(AgentSessionError::Failed);
        }
        if state["isStreaming"] == true
            || state["hasPendingAsyncWork"] == true
            || state["queuedMessageCount"]
                .as_u64()
                .is_some_and(|count| count > 0)
        {
            let turn = Uuid::new_v4().to_string();
            self.stream.begin(turn.clone());
            self.active = Some(turn.clone());
            self.phase = TurnPhase::AwaitingSettlement;
            self.prompt_id = None;
            return Ok(Prepared {
                prompt: None,
                native_turn: Some(turn),
            });
        }
        objective.clone_into(&mut input.text);
        Ok(Prepared {
            prompt: Some(input),
            native_turn: None,
        })
    }

    /// Read and control the goal through its native interface; never interpret it as model text.
    pub(super) async fn goal_control(
        &mut self,
        prompt: &AgentPrompt,
    ) -> Result<(), AgentSessionError> {
        let (_, args) = commands::split(&prompt.text).ok_or(AgentSessionError::Rejected)?;
        let op = match args {
            "" | "show" => "get",
            "pause" => "pause",
            "resume" => "resume",
            "drop" => "drop",
            _ => {
                self.stream.events.push_back(AgentTurnEvent::Timeline(commands::note(
                    &Uuid::new_v4().to_string(), "goal", true,
                    "This external OMP does not expose that goal operation through its public RPC. No goal was changed.",
                )));
                return Ok(());
            }
        };
        if self.vibe_host.as_ref().is_some_and(Host::enabled) && op == "resume" {
            return Err(AgentSessionError::Busy);
        }
        let result = self.transport.request("goal", json!({"op":op})).await?;
        self.confirm_goal(prompt, &result);
        Ok(())
    }

    /// Control the Goal currently displayed by the GUI, rejecting a stale target before mutation.
    pub(super) async fn current_goal_action(
        &mut self,
        arguments: &Value,
    ) -> Result<crate::ports::agent_session::AgentSessionFuture<'static, Value>, AgentSessionError>
    {
        let expected = arguments["expectedGoalId"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 1024)
            .ok_or(AgentSessionError::Rejected)?;
        let mut fields = arguments.clone();
        fields
            .as_object_mut()
            .ok_or(AgentSessionError::Rejected)?
            .remove("expectedGoalId");
        let fields = crate::local::omp::actions::validate("goal", &fields, &self.cwd)?;
        let op = fields["op"].as_str().ok_or(AgentSessionError::Rejected)?;
        if !matches!(op, "pause" | "resume" | "drop") {
            return Err(AgentSessionError::Rejected);
        }
        let current = self.transport.request("goal", json!({"op":"get"})).await?;
        if current["goal"]["id"] != expected {
            return Err(AgentSessionError::Rejected);
        }
        if op == "resume" && self.vibe_host.as_ref().is_some_and(Host::enabled) {
            return Err(AgentSessionError::Busy);
        }
        let result = self.transport.request("goal", fields).await?;
        self.confirm_goal(&AgentPrompt::text(""), &result);
        Ok(Box::pin(async { Ok(json!({"text":"Done","error":null})) }))
    }

    fn confirm_goal(&mut self, prompt: &AgentPrompt, result: &Value) {
        self.transport.supersede_buffered("goal_updated");
        self.goal_note(prompt, result);
    }

    /// Record mode changes, while keeping token-only updates out of the permanent timeline.
    pub(super) fn observe_goal(&mut self, state: &Value) {
        let previous = &self.state["goal"]["goal"];
        let goal = &state["goal"];
        if previous["id"] != goal["id"]
            || previous["status"] != goal["status"]
            || previous["objective"] != goal["objective"]
            || previous["tokenBudget"] != goal["tokenBudget"]
        {
            self.goal_note(
                &AgentPrompt::text("/goal show"),
                &json!({"goal":goal,"state":state}),
            );
        } else {
            self.state["goal"] = state.clone();
        }
    }

    pub(super) fn goal_note(&mut self, prompt: &AgentPrompt, result: &Value) {
        let id = prompt
            .client_message_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let note = goal_item(&id, result);
        self.state["goal"] = result["state"].clone();
        self.stream.events.push_back(AgentTurnEvent::Timeline(note));
    }
}

/// Build a current or historical Goal projection from acknowledged native facts.
pub(super) fn goal_item(id: &str, result: &Value) -> crate::protocol::timeline::NativeItem {
    let goal = &result["goal"];
    let status = goal["status"].as_str().unwrap_or("off");
    let text = if goal.is_object() {
        format!(
            "Goal {status}\n\n{}",
            goal["objective"].as_str().unwrap_or_default()
        )
    } else {
        "Goal off".to_owned()
    };
    let mut note = commands::note(id, "goal", false, &text);
    if goal.is_null() {
        note.item["goal"] = Value::Null;
    } else if let (Some(id), Some(objective), Some(phase)) = (
        goal["id"].as_str(),
        goal["objective"].as_str(),
        match status {
            "active" => Some("active"),
            "paused" => Some("paused"),
            "blocked" => Some("blocked"),
            "completed" => Some("complete"),
            _ => None,
        },
    ) {
        note.item["goal"] = json!({"provider":"omp","id":id,"objective":objective,"phase":phase});
        for key in ["tokensUsed", "tokenBudget"] {
            if let Some(value) = goal.get(key).filter(|value| value.is_number()) {
                note.item["goal"][key] = value.clone();
            }
        }
    }
    note.item["actions"] = match status {
        "active" => {
            json!([{"id":"pause","command":"/goal pause"},{"id":"clear","command":"/goal drop"}])
        }
        "paused" | "blocked" => {
            json!([{"id":"resume","command":"/goal resume"},{"id":"clear","command":"/goal drop"}])
        }
        "completed" => json!([{"id":"clear","command":"/goal drop"}]),
        _ => json!([]),
    };
    note
}

#[cfg(test)]
mod tests;
