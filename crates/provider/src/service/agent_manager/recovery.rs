use domain::agent_runtime::{AgentAttentionReason, AgentRuntimeStatus};
use serde_json::json;

use super::{AgentManager, AgentManagerError, map_registry};
use crate::protocol::timeline::NativeItem;
use crate::support::time::now_timestamp;

const INTERRUPTED: &str =
    "Execution was interrupted because the host restarted. Send Continue to resume.";

impl AgentManager {
    /// Retire persisted running turns that have no writer in this host process.
    ///
    /// Commits the diagnostic before the terminal snapshot and never replays native input.
    /// # Errors
    /// Returns a registry error if either durable write fails, so startup cannot serve stale work.
    pub(crate) fn recover_turns(&self) -> Result<(), AgentManagerError> {
        let timeline = self.timeline.as_ref().ok_or(AgentManagerError::Registry)?;
        for record in self.registry.list().map_err(map_registry)? {
            if record.last_status != AgentRuntimeStatus::Running
                || self.live.contains_key(&record.id)
            {
                continue;
            }
            let now = now_timestamp();
            let entry = NativeItem {
                key: format!(
                    "host:provider-failure:restart:{}",
                    record
                        .last_user_message_at
                        .as_ref()
                        .unwrap_or(&record.created_at)
                ),
                turn_id: None,
                timestamp: now.clone(),
                item: json!({"type":"error","message":INTERRUPTED}),
            };
            timeline
                .append(&record.id, &record.provider, &[entry])
                .map_err(|_| AgentManagerError::Registry)?;
            self.registry
                .update(&record.id, &|current| {
                    let mut next = current.clone();
                    if next.last_status == AgentRuntimeStatus::Running {
                        next.last_status = AgentRuntimeStatus::Error;
                        next.last_error = Some(INTERRUPTED.to_owned());
                        next.requires_attention = true;
                        next.attention_reason = Some(AgentAttentionReason::Error);
                        next.attention_timestamp = Some(now.clone());
                        next.updated_at.clone_from(&now);
                        next.last_activity_at = Some(now.clone());
                    }
                    next
                })
                .map_err(map_registry)?;
        }
        Ok(())
    }
}
