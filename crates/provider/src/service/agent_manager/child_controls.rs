//! Parent-owned typed guide/stop for observed native children.
//!
//! Controls execute through the parent's live managed writer; the child stays a read-only
//! observed record and never acquires a writer. Every request is validated against the parent
//! chain, live single-writer ownership and current observation health before any native write.

use domain::agent_runtime::observation::{ObservationHealth, ObservedStatus};

use super::{AgentManager, AgentManagerError, map_registry, map_session};
use crate::ports::controls::{ChildGuideOutcome, ChildStopOutcome};

/// A validated native child target resolved from an observed child record.
struct ChildTarget {
    /// Canonical native child identity used by the parent's adapter.
    native_id: String,
}

impl AgentManager {
    /// Guide one observed native child through its parent's live managed writer.
    ///
    /// # Errors
    /// Rejects a non-live or non-managed parent, a wrong-parent/stale/not-running child, blank
    /// guidance, or a native refusal. Never creates a child writer or resubmits uncertain input.
    pub(crate) async fn guide_child(
        &mut self,
        parent_id: &str,
        child_id: &str,
        message: &str,
        expected_revision: Option<u64>,
    ) -> Result<ChildGuideOutcome, AgentManagerError> {
        if message.trim().is_empty() {
            return Err(AgentManagerError::InvalidRequest);
        }
        let target = self.validate_child_target(parent_id, child_id, expected_revision, true)?;
        let agent = self
            .live
            .get_mut(parent_id)
            .ok_or(AgentManagerError::Busy)?;
        agent
            .session
            .guide_child(&target.native_id, message)
            .await
            .map_err(map_session)
    }

    /// Stop one observed native child through its parent's live managed writer.
    ///
    /// # Errors
    /// Rejects a non-live or non-managed parent or a wrong-parent/stale child. A non-running
    /// child yields `NotRunning` without error; acceptance is not proof of termination.
    pub(crate) async fn stop_child(
        &mut self,
        parent_id: &str,
        child_id: &str,
        expected_revision: Option<u64>,
    ) -> Result<ChildStopOutcome, AgentManagerError> {
        let target = self.validate_child_target(parent_id, child_id, expected_revision, false)?;
        let agent = self
            .live
            .get_mut(parent_id)
            .ok_or(AgentManagerError::Busy)?;
        agent
            .session
            .stop_child(&target.native_id)
            .await
            .map_err(map_session)
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
        // The parent must be a managed, unarchived session with a live writer in this process.
        if parent.observation.is_some()
            || parent.archived_at.is_some()
            || parent.internal
            || self.live_snapshot(parent_id).is_none()
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
            || !observation.ready
            || observation.health != ObservationHealth::Current
        {
            return Err(AgentManagerError::Busy);
        }
        if let Some(expected) = expected_revision {
            if observation.revision != expected {
                return Err(AgentManagerError::Busy);
            }
        }
        if require_running && observation.status != ObservedStatus::Running {
            return Err(AgentManagerError::Busy);
        }
        Ok(ChildTarget {
            native_id: observation.native_id.clone(),
        })
    }
}
