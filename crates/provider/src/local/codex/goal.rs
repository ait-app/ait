//! Native Codex Goal admission read over the public `thread/goal/get` RPC.
//!
//! The adapter owns native interpretation: status strings and error messages never leak
//! to the host as guesses. `Ok(None)` proves absence; an unavailable or failed probe is
//! never evidence of absence. Runtime error taxonomy is proven for codex-cli 0.167.0
//! (TAI loopback probe j31-codex-goalpause-loopback-cont): goal errors are -32600 with
//! exact messages, never -32601.

use domain::agent_runtime::goal::{GoalState, NativeGoalFacts};
use serde_json::{Value, json};

use super::CodexSession;
use crate::ports::agent_session::AgentSessionError;

/// Exact native message proving the goals feature is disabled (probe: -32600, never -32601).
const FEATURE_DISABLED: &str = "goals feature is disabled";

/// Normalize one native `ThreadGoal` payload into typed host facts.
pub(super) fn facts(goal: &Value) -> Result<NativeGoalFacts, AgentSessionError> {
    let status = goal["status"].as_str().ok_or(AgentSessionError::Failed)?;
    let objective = goal["objective"]
        .as_str()
        .ok_or(AgentSessionError::Failed)?;
    let state = match status {
        "active" => GoalState::Active,
        "paused" => GoalState::Paused,
        "blocked" => GoalState::Blocked,
        "complete" => GoalState::Completed,
        // Native rate/budget limits need user intervention; they are displayed facts,
        // never host-enforced caps (probe: budget overshoot 26 > 13).
        "usageLimited" | "budgetLimited" => GoalState::Blocked,
        _ => return Err(AgentSessionError::Failed),
    };
    let meters = json!({
        "tokensUsed": goal["tokensUsed"],
        "timeUsedSeconds": goal["timeUsedSeconds"],
        "tokenBudget": goal["tokenBudget"],
    });
    Ok(NativeGoalFacts {
        state,
        objective: objective.to_owned(),
        native_id: None,
        native_status: status.to_owned(),
        revision: goal["updatedAt"].as_u64().map(|value| value.to_string()),
        meters: Some(meters),
    })
}

/// Classify a goal RPC rejection by its exact native message (probe taxonomy).
fn rejection(message: &str) -> AgentSessionError {
    if message == FEATURE_DISABLED {
        AgentSessionError::Unavailable
    } else {
        AgentSessionError::Rejected
    }
}

impl CodexSession {
    /// Read native goal facts without mutation; `Ok(None)` proves absence.
    /// Admission needs an authoritative current read, not a cached native state system:
    /// this path issues one `thread/goal/get` and projects the response, nothing else.
    pub(super) async fn goal_read(&mut self) -> Result<Option<NativeGoalFacts>, AgentSessionError> {
        if self.history {
            return Err(AgentSessionError::Unavailable);
        }
        let transport = self.transport.as_mut().ok_or(AgentSessionError::Failed)?;
        let response = transport
            .request("thread/goal/get", json!({"threadId": self.id}))
            .await
            .map_err(|error| match error {
                AgentSessionError::Rejected => rejection(
                    transport
                        .rpc_error
                        .as_ref()
                        .map(secrecy::ExposeSecret::expose_secret)
                        .unwrap_or_default(),
                ),
                other => other,
            })?;
        let goal = response.get("goal").ok_or(AgentSessionError::Failed)?;
        if goal.is_null() {
            return Ok(None);
        }
        facts(goal).map(Some)
    }
}

#[cfg(test)]
mod tests;
