//! Creation categories, subscription requests and committed progress values.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Resource whose creation is observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Workspace provisioning.
    Workspace,
    /// Native Agent provisioning.
    Agent,
}

impl Kind {
    /// Return the wire name used by receipts and update events.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::Agent => "agent",
        }
    }
}

/// Committed phase of a Workspace or Agent creation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// The creation intent was accepted and resource identities were reserved.
    Accepted,
    /// The Workspace resource is ready for an embedded Agent.
    WorkspaceReady,
    /// The Agent resource is registered and ready for its initial prompt.
    AgentReady,
    /// The initial prompt was submitted to the Agent.
    PromptStarted,
    /// The creation completed successfully.
    Completed,
    /// The creation failed.
    Failed,
}

impl Phase {
    /// Whether this phase ends the creation attempt.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }
}

/// Observe one idempotency key, optionally without retaining a subscription.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscribeRequest {
    /// Resource kind.
    pub kind: Kind,
    /// Explicit durable key supplied to creation.
    pub idempotency_key: String,
    /// Defaults to true.
    pub subscribe: Option<bool>,
}

/// Last committed progress of one creation intent.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// Resource kind.
    pub kind: Kind,
    /// Original durable key.
    pub idempotency_key: String,
    /// Monotonically increasing committed revision.
    pub revision: u64,
    /// Current creation phase.
    pub phase: Phase,
    /// Reserved or completed Workspace ID.
    pub workspace_id: Option<String>,
    /// Reserved or completed Agent ID.
    pub agent_id: Option<String>,
    /// Safe terminal error, never raw native diagnostics.
    pub error: Option<String>,
    /// Whether an interrupted attempt may have created native resources.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub outcome_unknown: bool,
    /// Completed Workspace projection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<Value>,
    /// Completed Agent projection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<Value>,
}

impl Snapshot {
    /// Return the owned Workspace and Agent IDs. An Agent creation borrows its Workspace.
    #[must_use]
    pub fn resource_claims(&self) -> (Option<&str>, Option<&str>) {
        let workspace = self
            .workspace_id
            .as_deref()
            .filter(|_| self.kind == Kind::Workspace);
        (workspace, self.agent_id.as_deref())
    }
}

#[cfg(test)]
mod tests;
