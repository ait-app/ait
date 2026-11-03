//! Native child facts are separate from ownership of an interactive provider writer.

use serde::{Deserialize, Serialize};

use super::AgentPersistenceHandle;

/// Last confirmed native child execution state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservedStatus {
    /// Native execution is active.
    Running,
    /// Native execution reported successful completion.
    Completed,
    /// Native execution reported failure.
    Failed,
    /// Native execution reported cancellation.
    Canceled,
    /// Available native facts do not establish an execution state.
    Unknown,
}

impl ObservedStatus {
    /// Whether the native provider has confirmed an execution outcome.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Canceled)
    }
}

/// Availability of the channel that supplies native child facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationHealth {
    /// A current native observation established the state.
    Current,
    /// Persisted facts await native confirmation after a disconnect or restart.
    Stale,
    /// The original root or native source is unavailable.
    Unavailable,
}

/// Durable read-only binding to a verified descendant of a managed native session.
///
/// This locator never grants permission to resume or mutate a child independently.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedAgent {
    /// Monotonic host observation revision, independent of native timestamp precision.
    #[serde(default)]
    pub revision: u64,
    /// Managed Agent that owns the observation channel and legacy timeline scope.
    pub root_agent_id: String,
    /// Native root identity at registration, retained across root rewind.
    pub root_session_id: String,
    /// Verified immediate parent expressed as an Ait Agent identity.
    pub parent_agent_id: String,
    /// Canonical native child identity.
    pub native_id: String,
    /// Verified immediate native parent.
    pub parent_native_id: String,
    /// Private adapter-specific history locator; never an interactive resume handle.
    pub persistence: Option<AgentPersistenceHandle>,
    /// Whether the durable timeline binding has committed.
    pub ready: bool,
    /// Last confirmed native execution state.
    pub status: ObservedStatus,
    /// Observation-channel health, independent of the execution outcome.
    pub health: ObservationHealth,
    /// Native observation timestamp, retained when health becomes stale.
    pub observed_at: String,
    /// Provider task description used to distinguish siblings.
    pub description: Option<String>,
    /// Compact provider-owned display context.
    pub subtitle: Option<String>,
}

#[cfg(test)]
mod tests;
