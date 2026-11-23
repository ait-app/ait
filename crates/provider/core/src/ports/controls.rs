//! Provider-owned child identity; host Agents remain separate registration records.

use serde_json::Value;

/// A discovered native descendant and its verified immediate-parent link.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NativeSubagent {
    /// Optional provider locator for children whose native transcript is not a root session.
    pub persistence: Option<domain::agent_runtime::AgentPersistenceHandle>,
    /// Native child identity.
    pub id: String,
    /// Native immediate parent, used to enforce root ancestry.
    pub parent_id: String,
    /// Native directory, used only after the parent relationship is validated.
    pub cwd: String,
    /// Paseo descriptor fields except the host parentAgentId and parentSubagentId.
    pub descriptor: Value,
}

/// Provider-owned child updates, isolated from the root timeline and foreground turn.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SubagentEvent {
    /// Complete descriptor with a verified immediate-parent link.
    Upsert(NativeSubagent),
    /// A retry-stable incremental observation in the child's own timeline.
    Progress {
        /// Canonical provider child identity.
        id: String,
        /// Unique native observation identity.
        observation: String,
        /// Text delta or running tool snapshot.
        entry: crate::protocol::timeline::NativeItem,
    },
    /// A completed immutable child timeline item.
    Timeline {
        /// Canonical provider child identity.
        id: String,
        /// Completed source item.
        entry: crate::protocol::timeline::NativeItem,
    },
}

/// Correlation returned when a parent-owned control is queued for native delivery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildControlReceipt {
    /// Adapter-generated identity of the eventual control outcome.
    pub token: String,
}

/// Native result of sending guidance to a child.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ChildGuideOutcome {
    /// Guidance was accepted by the parent-owned native runtime.
    Accepted,
    /// The target child is not running, so no guidance was delivered.
    NotRunning,
}

/// Native result of requesting one child stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ChildStopOutcome {
    /// The native runtime accepted the stop; termination is observed separately.
    Cancelled,
    /// The child is no longer running and nothing was stopped.
    NotRunning,
}

/// Why a child control did not establish a native disposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ChildControlFailure {
    /// The provider definitively refused the control.
    Rejected,
    /// Delivery could not be determined; the operation must never be automatically replayed.
    Uncertain,
}

/// Correlated outcome of one parent-owned native child control.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ChildControlOutcome {
    /// Outcome of sending guidance.
    Guide(Result<ChildGuideOutcome, ChildControlFailure>),
    /// Outcome of requesting stop.
    Stop(Result<ChildStopOutcome, ChildControlFailure>),
}
