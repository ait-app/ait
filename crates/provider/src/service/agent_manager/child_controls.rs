//! Parent-owned typed guide/stop for observed native children.
//!
//! Controls execute through the parent's live managed writer; the child stays a read-only
//! observed record and never acquires a writer. Every request is validated against the parent
//! chain, live single-writer ownership and current observation health before any native write.

use domain::agent_runtime::observation::{ObservationHealth, ObservedStatus};

use super::{AgentManager, AgentManagerError, map_registry, map_session};
use crate::ports::controls::{ChildControlOutcome, ChildControlReceipt};

/// Bound on child controls awaiting a native answer per parent Agent.
pub(super) const LIVE_CHILD_CONTROLS: usize = 8;

/// A validated native child target resolved from an observed child record.
struct ChildTarget {
    /// Canonical native child identity used by the parent's adapter.
    native_id: String,
}

/// One child control already written to the native runtime, awaiting its typed answer.
///
/// The control leaves this process as a single native command; its outcome is published by the
/// parent's ordinary event drain, so the caller keeps the receipt and the parent lane stays
/// available for queued permissions and polls in the meantime.
pub(crate) struct DispatchedControl {
    /// Adapter receipt token, echoed by the eventual outcome.
    pub(crate) receipt: ChildControlReceipt,
}

impl AgentManager {
    /// Dispatch guidance for one observed native child through its parent's live managed writer.
    ///
    /// # Errors
    /// Rejects a non-live or non-managed parent, a wrong-parent/stale/not-running child, blank
    /// guidance, a provider without native child guidance, or an exhausted pending-control
    /// budget. Never creates a child writer or resubmits uncertain input.
    pub(crate) async fn guide_child(
        &mut self,
        parent_id: &str,
        child_id: &str,
        message: &str,
        expected_revision: Option<u64>,
    ) -> Result<DispatchedControl, AgentManagerError> {
        if message.trim().is_empty() {
            return Err(AgentManagerError::InvalidRequest);
        }
        let target = self.validate_child_target(parent_id, child_id, expected_revision, true)?;
        let agent = self.control_writer(parent_id)?;
        let receipt = agent
            .session
            .guide_child(&target.native_id, message)
            .await
            .map_err(map_session)?;
        Ok(Self::track_child_control(agent, receipt))
    }

    /// Dispatch a stop for one observed native child through its parent's live managed writer.
    ///
    /// # Errors
    /// Rejects a non-live or non-managed parent, a wrong-parent/stale child, a provider without
    /// native child stop, or an exhausted pending-control budget. A non-running child yields
    /// `NotRunning` as a typed answer; acceptance is not proof of termination.
    pub(crate) async fn stop_child(
        &mut self,
        parent_id: &str,
        child_id: &str,
        expected_revision: Option<u64>,
    ) -> Result<DispatchedControl, AgentManagerError> {
        let target = self.validate_child_target(parent_id, child_id, expected_revision, false)?;
        let agent = self.control_writer(parent_id)?;
        let receipt = agent
            .session
            .stop_child(&target.native_id)
            .await
            .map_err(map_session)?;
        Ok(Self::track_child_control(agent, receipt))
    }

    /// Borrow the parent's live writer while enforcing the pending-control budget.
    fn control_writer(
        &mut self,
        parent_id: &str,
    ) -> Result<&mut super::LiveAgent, AgentManagerError> {
        let agent = self
            .live
            .get_mut(parent_id)
            .ok_or(AgentManagerError::Busy)?;
        if agent.child_controls.len() >= LIVE_CHILD_CONTROLS {
            return Err(AgentManagerError::Busy);
        }
        Ok(agent)
    }

    /// Retain the deferred answer slot for one dispatched control.
    fn track_child_control(
        agent: &mut super::LiveAgent,
        receipt: ChildControlReceipt,
    ) -> DispatchedControl {
        agent.child_controls.insert(receipt.token.clone(), None);
        DispatchedControl { receipt }
    }

    /// Take every child control answer committed by the parent's event drains.
    ///
    /// Callers poll this from the same lane that dispatched the control, so the reply settles
    /// without holding the writer while the native runtime answers.
    pub(crate) fn take_completed_child_controls(&mut self) -> Vec<(String, ChildControlOutcome)> {
        let mut completed = Vec::new();
        for agent in self.live.values_mut() {
            let ready = agent
                .child_controls
                .iter()
                .filter(|(_, outcome)| outcome.is_some())
                .map(|(token, _)| token.clone())
                .collect::<Vec<_>>();
            for token in ready {
                if let Some(Some(outcome)) = agent.child_controls.remove(&token) {
                    completed.push((token, outcome));
                }
            }
        }
        completed
    }

    /// Validate parent ownership and the observed child chain before any native side effect.
    ///
    /// `require_running` gates guidance to running children; stop accepts the native runtime's
    /// own `not_running` fact instead of pre-rejecting a terminal child.
    fn validate_child_target(
        &self,
        parent_id: &str,
        child_id: &str,
        expected_revision: Option<u64>,
        require_running: bool,
    ) -> Result<ChildTarget, AgentManagerError> {
        let parent = self
            .registry
            .get(parent_id)
            .map_err(map_registry)?
            .ok_or_else(|| AgentManagerError::NotFound(parent_id.to_owned()))?;
        let live_parent = self
            .live_snapshot(parent_id)
            .ok_or(AgentManagerError::Busy)?;
        // The parent must be a managed, unarchived session with a live writer in this process.
        if parent.observation.is_some()
            || parent.archived_at.is_some()
            || parent.internal
            || parent.provider != live_parent.provider
        {
            return Err(AgentManagerError::Busy);
        }
        let child = self
            .registry
            .get(child_id)
            .map_err(map_registry)?
            .ok_or_else(|| AgentManagerError::NotFound(child_id.to_owned()))?;
        let observation = child
            .observation
            .as_ref()
            .ok_or(AgentManagerError::InvalidRequest)?;
        // Wrong-parent and stale-observation requests are rejected without disclosing the target.
        if observation.parent_agent_id != parent_id
            || observation.root_agent_id != parent_id
            || child.provider != live_parent.provider
            || !live_parent.persistence.as_ref().is_some_and(|handle| {
                handle.provider == child.provider
                    && handle.session_id == observation.root_session_id
            })
            || !observation.ready
            || observation.health != ObservationHealth::Current
        {
            return Err(AgentManagerError::Busy);
        }
        if let Some(expected) = expected_revision
            && observation.revision != expected
        {
            return Err(AgentManagerError::Busy);
        }
        if require_running && observation.status != ObservedStatus::Running {
            return Err(AgentManagerError::Busy);
        }
        Ok(ChildTarget {
            native_id: observation.native_id.clone(),
        })
    }
}
