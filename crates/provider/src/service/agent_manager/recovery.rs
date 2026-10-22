use domain::agent_runtime::{
    AgentAttentionReason, AgentRuntimeStatus, PersistedAgentRuntimeRecord,
};
use serde_json::json;
use sha2::{Digest, Sha256};

use super::{AgentManager, AgentManagerError, map_registry};
use crate::protocol::prompt::AgentPrompt;
use crate::protocol::timeline::NativeItem;
use crate::storage::timeline::{Timeline, inbox::Receipt};
use crate::support::time::now_timestamp;

const INTERRUPTED: &str = concat!(
    "Execution was interrupted because the host restarted. ",
    "Automatic recovery was not possible. Send Continue to retry."
);
pub(super) const RESTART_INPUT_PREFIX: &str = "host:restart:";
const CONTINUE: &str = concat!(
    "The host restarted unexpectedly and interrupted this task. ",
    "Continue the previous task from the saved conversation. ",
    "Verify any unfinished tool operation before repeating it. ",
    "If the task is already complete, report the result without repeating completed actions."
);

impl AgentManager {
    /// Queue a continuation for resumable Codex work and fail other interrupted turns.
    ///
    /// The durable FIFO submits a separate continuation through the normal session lane; the
    /// interrupted user input is never replayed. Failed or uncertain recovery is reported once.
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
            if self.queue_restart_continuation(&record, timeline)? {
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

    fn queue_restart_continuation(
        &self,
        record: &PersistedAgentRuntimeRecord,
        timeline: &Timeline,
    ) -> Result<bool, AgentManagerError> {
        if record.provider != "codex" || record.internal || record.archived_at.is_some() {
            return Ok(false);
        }
        let Some(client) = self.clients.get(&record.provider) else {
            return Ok(false);
        };
        let Some(handle) = record.persistence.as_ref().filter(|handle| {
            handle.provider == record.provider && !handle.session_id.trim().is_empty()
        }) else {
            return Ok(false);
        };
        if !client.persisted_permissions(handle).is_empty() {
            return Ok(false);
        }
        if timeline
            .has_queued_input(&record.id)
            .map_err(|_| AgentManagerError::Registry)?
        {
            return Ok(true);
        }
        let identity = serde_json::to_vec(&(
            &handle.session_id,
            record
                .last_user_message_at
                .as_ref()
                .unwrap_or(&record.created_at),
        ))
        .map_err(|_| AgentManagerError::Registry)?;
        let message = format!("{RESTART_INPUT_PREFIX}{:x}", Sha256::digest(identity));
        let mut prompt = AgentPrompt::text(CONTINUE);
        prompt.client_message_id = Some(message.clone());
        match timeline.reserve_input(&record.id, &message, &prompt, "interrupt") {
            Ok(Receipt::New) => Ok(true),
            Ok(Receipt::Accepted | Receipt::Uncertain | Receipt::Rejected)
            | Err(model::ErrorCode::ResourceExhausted) => Ok(false),
            Err(_) => Err(AgentManagerError::Registry),
        }
    }
}
