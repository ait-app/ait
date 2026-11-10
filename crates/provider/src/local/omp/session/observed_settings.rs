//! Native plugin configuration facts share the application's validated configuration commit path.

use serde_json::Value;

use super::{AgentSessionError, AgentTurnEvent, Session, config};
use crate::protocol::agent_config::{ConfigPatch, NullableSetting};

/// Coalesce native change events until a snapshot has been scheduled on the existing writer.
#[derive(Debug, Default)]
pub(super) enum Refresh {
    #[default]
    Current,
    Pending,
}

impl Refresh {
    /// Whether a fresh native snapshot still needs scheduling.
    pub(super) fn pending(&self) -> bool {
        matches!(self, Self::Pending)
    }
    /// Mark a native configuration observation as pending.
    pub(super) fn mark(&mut self) {
        *self = Self::Pending;
    }
    /// Transfer the pending observation into the correlated state request.
    pub(super) fn take(&mut self) -> bool {
        std::mem::take(self).pending()
    }
}

impl Session {
    /// Apply only acknowledged native model/thinking fields; malformed native facts fail closed.
    pub(super) fn observe_settings(&mut self, event: &Value) -> Result<(), AgentSessionError> {
        let mut state = self.state.clone();
        if event.get("model").is_some() {
            state["model"] = event["model"].clone();
        }
        if event.get("thinkingLevel").is_some() {
            state["thinkingLevel"] = event["thinkingLevel"].clone();
        }
        let mut info = config::runtime(&state)?;
        let previous = config::runtime(&self.state)?;
        if info.model == previous.model && info.thinking_option_id == previous.thinking_option_id {
            self.state = state;
            self.stream.set_context(&self.state);
            return Ok(());
        }
        info.mode_id.clone_from(&self.config.mode_id);
        let patch = ConfigPatch {
            model_id: if event.get("model").is_some() {
                info.model
                    .clone()
                    .map_or(NullableSetting::Clear, NullableSetting::Set)
            } else {
                NullableSetting::Unchanged
            },
            thinking_option_id: if event.get("thinkingLevel").is_some() {
                info.thinking_option_id
                    .clone()
                    .map_or(NullableSetting::Clear, NullableSetting::Set)
            } else {
                NullableSetting::Unchanged
            },
            ..ConfigPatch::default()
        };
        let next = patch.apply(&self.config);
        config::validate(&next)?;
        self.config = next;
        self.state = state;
        self.stream.set_context(&self.state);
        self.stream
            .events
            .push_back(AgentTurnEvent::ModelSelectionUpdated {
                model: info.model.clone(),
                thinking_option_id: info.thinking_option_id.clone(),
            });
        self.stream
            .events
            .push_back(AgentTurnEvent::RuntimeInfo(info));
        Ok(())
    }
}

#[cfg(test)]
mod tests;
