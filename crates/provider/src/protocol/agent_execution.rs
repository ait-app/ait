//! Native Agent execution requests using Paseo's canonical method and field names.

use std::collections::BTreeMap;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use serde::Deserialize;

/// Native session configuration. Advanced fields are validated before any side effect.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionConfig {
    /// Identity of a registered native provider adapter.
    pub(crate) provider: String,
    /// Absolute existing directory.
    pub(crate) cwd: String,
    /// Optional display title.
    pub(crate) title: Option<String>,
    /// Persistable native configuration.
    #[serde(flatten)]
    pub(crate) stored: StoredAgentConfig,
}

/// Create an independently owned native session.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateRequest {
    /// Durable key for retries and creation observers.
    pub(crate) idempotency_key: Option<String>,
    /// Optional first text turn, submitted after native registration commits.
    pub(crate) initial_prompt: Option<String>,
    /// Initial inline raster images.
    #[serde(default)]
    pub(crate) images: Vec<super::prompt::PromptImage>,
    /// Initial contextual attachments.
    #[serde(default)]
    pub(crate) attachments: Vec<serde_json::Value>,
    /// Optional identity for the initial user input.
    pub(crate) client_message_id: Option<String>,
    /// Optional native structured output constraint.
    pub(crate) output_schema: Option<serde_json::Value>,
    /// Optional caller-selected UUID.
    pub(crate) agent_id: Option<String>,
    /// Native configuration.
    pub(crate) config: SessionConfig,
    /// Ephemeral native child environment, excluded from Agent persistence and responses.
    #[serde(default)]
    pub(crate) env: crate::ports::environment::AgentEnvironment,
    /// Existing active Workspace whose directory replaces the draft directory.
    pub(crate) workspace_id: Option<String>,
    /// Parent Agent used for inherited Workspace placement and parentage labeling.
    pub(crate) caller_agent_id: Option<String>,
    /// Archive this Agent after its first native completed, failed, or cancelled turn.
    #[serde(default)]
    pub(crate) auto_archive: bool,
    /// Managed worktree selection that takes precedence over existing placement.
    pub(crate) worktree: Option<super::creation::WorktreeTarget>,
    /// Legacy Git placement, mutually exclusive with `worktree`.
    pub(crate) git: Option<super::creation::GitOptions>,
    /// Legacy name for a new managed worktree when `git` is omitted.
    pub(crate) worktree_name: Option<String>,
    /// Initial public labels.
    #[serde(default)]
    pub(crate) labels: BTreeMap<String, String>,
}

/// Restore a native identity without creating a new provider history.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ResumeRequest {
    /// Native identity; unknown handles require a cwd in metadata or overrides.
    pub(crate) handle: AgentPersistenceHandle,
    /// Draft settings applied atomically when the interactive native session is restored.
    #[serde(default)]
    pub(crate) overrides: super::resume::Overrides,
}

/// Explicit delivery policy for an already active native turn.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ActiveTurnBehavior {
    /// Interrupt the active turn and deliver this input after its terminal acknowledgement.
    #[default]
    Interrupt,
    /// Ask the provider to admit text into the current turn, without interrupting it.
    Steer,
}

/// Submit text, optionally steering the active native turn.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SendRequest {
    /// Full ID, unambiguous prefix, or exact title.
    pub(crate) agent_id: String,
    /// Nonempty text, at most 64 KiB.
    text: String,
    /// Omission interrupts ordinary foreground work; voice-owned turns remain exclusive.
    pub(crate) active_turn_behavior: Option<ActiveTurnBehavior>,
    /// Client retry identity (Paseo's `messageId`).
    message_id: Option<String>,
    /// Inline raster images.
    #[serde(default)]
    images: Vec<super::prompt::PromptImage>,
    /// Contextual attachments.
    #[serde(default)]
    attachments: Vec<serde_json::Value>,
    /// Optional structured output constraint.
    output_schema: Option<serde_json::Value>,
}

impl SendRequest {
    /// Consume the request's rich input after the caller resolves its Agent and delivery policy.
    #[must_use]
    pub(crate) fn into_prompt(self) -> super::prompt::AgentPrompt {
        super::prompt::AgentPrompt {
            text: self.text,
            images: self.images,
            attachments: self.attachments,
            client_message_id: self.message_id,
            output_schema: self.output_schema,
        }
    }
}

/// Await native completion without occupying the Provider command lane.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WaitRequest {
    /// Full ID, unambiguous prefix, or exact title.
    pub(crate) agent_id: String,
    /// Positive timeout in milliseconds; omission waits until completion or caller cancellation.
    pub(crate) timeout_ms: Option<u64>,
}
