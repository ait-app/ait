//! Native configuration facts: a full `get_state` snapshot is an observation only (it never
//! creates an override), while explicit native change events are accepted selection updates
//! that persist so they survive reopen. Pre-acknowledgement staleness is fenced by superseding
//! buffered selection events after an Ait set/readback (see `Session::apply_live_fields`).

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
    /// Apply native configuration facts; malformed model/thinking values fail closed.
    ///
    /// A full `get_state` snapshot (carries `sessionId`) is an observation: it refreshes the
    /// stored state and runtime facts but never converts an inherited `None` into an explicit
    /// override, and never reverts a stored override. An explicit native change event
    /// (`config_update` / `model_changed` / `thinking_level_changed`) is an accepted selection
    /// update: it persists the actual model/thinking so the change survives reopen/resume.
    pub(super) fn observe_settings(&mut self, event: &Value) -> Result<(), AgentSessionError> {
        let is_snapshot = event.get("sessionId").is_some();
        let state = if is_snapshot {
            event.clone()
        } else {
            let mut merged = self.state.clone();
            if event.get("model").is_some() {
                merged["model"] = event["model"].clone();
            }
            if event.get("thinkingLevel").is_some() {
                merged["thinkingLevel"] = event["thinkingLevel"].clone();
            }
            merged
        };
        let mut info = config::runtime(&state)?;
        let previous = config::runtime(&self.state)?;
        if info.model == previous.model && info.thinking_option_id == previous.thinking_option_id {
            self.state = state;
            self.stream.set_context(&self.state);
            return Ok(());
        }
        info.mode_id.clone_from(&self.config.mode_id);
        if is_snapshot {
            // Observation only: refresh state and runtime facts, keep the stored override.
            self.state = state;
            self.stream.set_context(&self.state);
            self.stream
                .events
                .push_back(AgentTurnEvent::RuntimeInfo(info));
            return Ok(());
        }
        // Explicit native change: persist the accepted selection through the single writer.
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
                model: patch.model_id,
                thinking_option_id: patch.thinking_option_id,
            });
        self.stream
            .events
            .push_back(AgentTurnEvent::RuntimeInfo(info));
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests;
