//! Provider-owned child identity; host Agents remain separate registration records.

use serde_json::Value;

/// A discovered native descendant and its verified immediate-parent link.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct NativeSubagent {
    /// Optional provider locator for children whose native transcript is not a root session.
    pub(crate) persistence: Option<domain::agent_runtime::AgentPersistenceHandle>,
    /// Native child identity.
    pub(crate) id: String,
    /// Native immediate parent, used to enforce root ancestry.
    pub(crate) parent_id: String,
    /// Native directory, used only after the parent relationship is validated.
    pub(crate) cwd: String,
    /// Paseo descriptor fields except the host parentAgentId and parentSubagentId.
    pub(crate) descriptor: Value,
}

/// Provider-owned child updates, isolated from the root timeline and foreground turn.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) enum SubagentEvent {
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

/// Typed outcome of guiding one native child through its parent's managed writer.
///
/// Guidance is a steering user message delivered into the child's own native session by the
/// parent's RPC process; it never creates a child writer. Only a running or pending child can
/// accept guidance (verified OMP `steer_subagent` semantics).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChildGuideOutcome {
    /// The native runtime accepted the steering message into the running child.
    Accepted,
    /// The child is not running/pending, so no message was delivered. A typed fact, not success.
    NotRunning,
}

/// Typed outcome of stopping one native child through its parent's managed writer.
///
/// Stop is a hard-kill of a single running child inside the native runtime; the parent turn and
/// sibling children are unaffected. Acceptance is not proof of termination: the observed terminal
/// state arrives later through the read-only child lifecycle pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChildStopOutcome {
    /// The native runtime accepted the stop (`cancelled: true`). Termination is observed separately.
    Cancelled,
    /// Nothing was cancelled because the child was not running (`cancelled: false`). Never success.
    NotRunning,
}
