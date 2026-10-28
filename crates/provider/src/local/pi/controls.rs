//! Pure planning and projection for Pi's native model and thinking controls.

use domain::agent_runtime::StoredAgentConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::config;
use crate::ports::agent_session::AgentSessionError;
use crate::protocol::agent_config::{ConfigPatch, NullableSetting};

/// Saved preferences whose native restoration was skipped for an imminent patch.
#[derive(Debug, Default)]
pub(super) struct SuppressedControls {
    /// Keep the saved model until its replacement succeeds.
    pub(super) model: bool,
    /// Keep the saved thinking preference until a native control replaces it.
    pub(super) thinking: bool,
}

/// A successful setup-only reset must survive writers until Pi stores native input.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResetIntent {
    /// No reset needs restoring on another setup-only writer.
    #[default]
    None,
    /// Restore the one-shot reset when reopening without native conversation history.
    UntilInput,
}

/// Durable next-turn intent; native runtime facts remain separate until execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct DeferredConfig {
    pub(super) config: StoredAgentConfig,
    /// Last applied preferences, so rejected recovery does not adopt queued choices.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) previous_config: Option<StoredAgentConfig>,
    #[serde(default)]
    model: bool,
    #[serde(default)]
    thinking: bool,
}

impl DeferredConfig {
    /// Merge the latest patch, retaining only controls not superseded by a model switch.
    pub(super) fn merge(
        previous: Option<&Self>,
        applied: &StoredAgentConfig,
        config: &StoredAgentConfig,
        patch: &ConfigPatch,
    ) -> Self {
        let model_switch = matches!(patch.model_id, NullableSetting::Set(_));
        let mut config = config.clone();
        if model_switch && patch.thinking_option_id == NullableSetting::Unchanged {
            config.thinking_option_id = None;
        }
        Self {
            config,
            previous_config: previous
                .and_then(|pending| pending.previous_config.clone())
                .or_else(|| Some(applied.clone())),
            model: patch.model_id != NullableSetting::Unchanged
                || previous.is_some_and(|pending| pending.model),
            thinking: patch.thinking_option_id != NullableSetting::Unchanged
                || !model_switch && previous.is_some_and(|pending| pending.thinking),
        }
    }

    /// Reconstruct explicit native commands, including one-time clearing semantics.
    pub(super) fn patch(&self) -> ConfigPatch {
        ConfigPatch {
            model_id: if self.model {
                self.config
                    .model
                    .clone()
                    .map_or(NullableSetting::Clear, NullableSetting::Set)
            } else {
                NullableSetting::Unchanged
            },
            thinking_option_id: if self.thinking {
                self.config
                    .thinking_option_id
                    .clone()
                    .map_or(NullableSetting::Clear, NullableSetting::Set)
            } else {
                NullableSetting::Unchanged
            },
            ..ConfigPatch::default()
        }
    }

    /// Whether the durable command releases thinking to the one-shot Medium baseline.
    pub(super) fn clears_thinking(&self) -> bool {
        self.thinking && self.config.thinking_option_id.is_none()
    }
}

/// The caller's intent determines whether an unchanged selection must be sent again.
#[derive(Debug, Clone, Copy)]
pub(super) enum Intent<'a> {
    /// Creation/resume applies explicit thinking while preserving an already selected model.
    Open,
    /// Configuration-only resume restores controls that no imminent command replaces.
    Reconcile { model: bool, thinking: bool },
    /// Explicit fields are commands, including reselections and clearing thinking.
    Patch(&'a ConfigPatch),
    /// Direct turn callers may supply configuration that differs from their last request.
    Turn,
}

/// Ordered native mutations borrowing their values from the requested configuration.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Plan<'a> {
    /// Qualified model split into native provider and model ID, when selection is required.
    pub(super) model: Option<(&'a str, &'a str)>,
    /// Native thinking ID; clearing selects Pi's Medium baseline.
    pub(super) thinking: Option<&'a str>,
}

impl<'a> Plan<'a> {
    /// Plan `requested` against prior host intent and current native state.
    /// Returns rejection for an unrepresentable model identity.
    pub(super) fn new(
        requested: &'a StoredAgentConfig,
        previous: &StoredAgentConfig,
        state: &Value,
        intent: Intent<'_>,
    ) -> Result<Self, AgentSessionError> {
        let model = requested
            .model
            .as_deref()
            .map(|model| config::split_model(model).ok_or(AgentSessionError::Rejected))
            .transpose()?;
        let model_changed = match intent {
            Intent::Patch(patch) => patch.model_id != NullableSetting::Unchanged,
            Intent::Reconcile { model: false, .. } => false,
            Intent::Open | Intent::Turn | Intent::Reconcile { model: true, .. } => model
                .is_some_and(|(provider, id)| {
                    state["model"]["provider"] != provider || state["model"]["id"] != id
                }),
        };
        let thinking_changed = match intent {
            Intent::Open => requested.thinking_option_id.is_some(),
            Intent::Reconcile { thinking, .. } => {
                thinking && requested.thinking_option_id.is_some()
            }
            Intent::Patch(patch) => patch.thinking_option_id != NullableSetting::Unchanged,
            Intent::Turn => {
                previous.thinking_option_id != requested.thinking_option_id
                    && requested
                        .thinking_option_id
                        .as_deref()
                        .is_some_and(|level| Some(level) != state["thinkingLevel"].as_str())
            }
        };
        Ok(Self {
            model: model.filter(|_| model_changed),
            thinking: thinking_changed
                .then(|| requested.thinking_option_id.as_deref().unwrap_or("medium")),
        })
    }

    /// Whether executing this plan requires native readback, even after partial failure.
    pub(super) fn changes_native_state(&self) -> bool {
        self.model.is_some() || self.thinking.is_some()
    }
}

/// Effective controls projected without allocating runtime metadata or compatibility maps.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Selection {
    /// Native model qualified by its provider.
    pub(super) model: Option<String>,
    /// Native thinking vocabulary, including IDs absent from the current catalog.
    pub(super) thinking: Option<String>,
}

impl Selection {
    /// Read the effective controls from a native state snapshot.
    pub(super) fn read(state: &Value) -> Self {
        Self {
            model: state["model"]["provider"]
                .as_str()
                .zip(state["model"]["id"].as_str())
                .map(|(provider, id)| format!("{provider}/{id}")),
            thinking: state["thinkingLevel"]
                .as_str()
                .filter(|level| config::valid_thinking_level_id(level))
                .map(str::to_owned),
        }
    }
}
