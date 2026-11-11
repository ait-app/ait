//! Native writes only; the host owns strategy, receipt admission, and restart arbitration.

use std::borrow::Cow;

use domain::agent_runtime::goal::{
    GoalContinuationOwner, GoalControlOp, GoalState, NativeGoalFacts,
};
use serde_json::{Value, json};

use super::{facts, rejection};
use crate::local::codex::CodexSession;
use crate::ports::agent_session::AgentSessionError;
use crate::ports::goal::{GoalCapability, GoalMetering, GoalPauseSemantics};

impl CodexSession {
    /// Report eligibility from this admitted transport; release proof does not cover prereleases.
    pub(in crate::local::codex) fn native_goal_capability(&self) -> GoalCapability {
        let proven = !self.history
            && self
                .transport
                .as_ref()
                .and_then(|transport| transport.user_agent.as_deref())
                .and_then(|agent| agent.split_whitespace().next())
                .is_some_and(|identity| identity.ends_with("/0.167.0"));
        if !proven {
            return GoalCapability {
                reason: Some(Cow::Borrowed(
                    "Codex uniform Goal Pause is verified only for an admitted 0.167.0 session",
                )),
                ..GoalCapability::default()
            };
        }
        GoalCapability {
            native: true,
            continuation: GoalContinuationOwner::NativeDriver,
            pause: GoalPauseSemantics::DeferredFinish,
            checkpoint_tool: false,
            metering: GoalMetering::Reliable,
            reason: None,
        }
    }

    /// Execute one native Goal RPC, preserving capability-negative versus rejected outcomes.
    /// Transport failures remain uncertain to the host; this method never retries.
    pub(in crate::local::codex) async fn request_goal(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, AgentSessionError> {
        let transport = self.transport.as_mut().ok_or(AgentSessionError::Failed)?;
        transport.request(method, params).await.map_err(|error| {
            if error == AgentSessionError::Rejected {
                rejection(
                    transport
                        .rpc_error
                        .as_ref()
                        .map(secrecy::ExposeSecret::expose_secret)
                        .unwrap_or_default(),
                )
            } else {
                error
            }
        })
    }

    /// Create the admitted objective using user provenance; return acknowledged native facts.
    /// Returns unavailable for unproven runtimes or the native write error without retry.
    pub(in crate::local::codex) async fn create_native_goal(
        &mut self,
        objective: &str,
    ) -> Result<NativeGoalFacts, AgentSessionError> {
        self.set_native_goal(json!({
            "threadId": self.id, "objective": objective, "status": "active", "origin": "user"
        }))
        .await
    }

    /// Apply an admitted control without sending a prompt. Stop interrupts before disarming.
    /// Returns failed for a partially applied Stop so the host reconciles its receipt.
    pub(in crate::local::codex) async fn control_native_goal(
        &mut self,
        operation: GoalControlOp,
    ) -> Result<NativeGoalFacts, AgentSessionError> {
        if !self.native_goal_capability().native {
            return Err(AgentSessionError::Unavailable);
        }
        if operation == GoalControlOp::Clear {
            let mut previous = self.goal_read().await?.ok_or(AgentSessionError::Rejected)?;
            let response = self
                .request_goal(
                    "thread/goal/clear",
                    json!({"threadId": self.id, "origin": "user"}),
                )
                .await?;
            // A false acknowledgement means the native goal vanished between the read and the
            // clear. Reporting the stale pre-read facts as cleared would fabricate an outcome,
            // so the host must reconcile absence through the authoritative read instead.
            if !response["cleared"]
                .as_bool()
                .ok_or(AgentSessionError::Failed)?
            {
                return Err(AgentSessionError::Rejected);
            }
            self.pending_goal_start = false;
            previous.state = GoalState::Cleared;
            "cleared".clone_into(&mut previous.native_status);
            return Ok(previous);
        }
        let interrupted = operation == GoalControlOp::Stop && self.active_turn.is_some();
        if interrupted {
            self.transport
                .as_mut()
                .ok_or(AgentSessionError::Failed)?
                .request(
                    "turn/interrupt",
                    json!({"threadId": self.id, "turnId": self.active_turn}),
                )
                .await?;
        }
        let status = match operation {
            GoalControlOp::Resume => "active",
            GoalControlOp::Pause | GoalControlOp::Stop => "paused",
            GoalControlOp::Clear => unreachable!("Clear returned before the set operation"),
        };
        let result = self
            .set_native_goal(json!({"threadId": self.id, "status": status, "origin": "user"}))
            .await;
        // An acknowledged interruption followed by a failed pause is a partial mutation,
        // not a definitive rejection. The host must reconcile rather than retry Stop.
        if interrupted {
            result.map_err(|_| AgentSessionError::Failed)
        } else {
            result
        }
    }

    /// Re-arm an already reconciled native run on the host arbiter's sole decision.
    /// Returns the native response or an error; never creates its own continuation decision.
    pub(in crate::local::codex) async fn reassert_native_goal(
        &mut self,
    ) -> Result<NativeGoalFacts, AgentSessionError> {
        // Only the host's existing arbiter calls this after receipt/generation reconciliation.
        self.set_native_goal(
            json!({"threadId": self.id, "status": "active", "origin": "automatic"}),
        )
        .await
    }

    async fn set_native_goal(
        &mut self,
        params: Value,
    ) -> Result<NativeGoalFacts, AgentSessionError> {
        if !self.native_goal_capability().native {
            return Err(AgentSessionError::Unavailable);
        }
        let response = self.request_goal("thread/goal/set", params).await?;
        let current = facts(response.get("goal").ok_or(AgentSessionError::Failed)?)?;
        self.pending_goal_start = current.state == GoalState::Active && self.active_turn.is_none();
        Ok(current)
    }
}

#[cfg(test)]
mod tests;
