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

/// A dispatched native child control whose final outcome arrives later.
///
/// The adapter writes exactly one native command and returns this receipt immediately; the
/// native answer is read by the ordinary event loop and published as
/// [`crate::ports::agent_session::AgentTurnEvent::ChildControl`] tagged with `token`. Waiting
/// inline for that answer would hold the parent's single-writer lane and delay queued
/// permission answers, so the receipt is the only synchronous result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChildControlReceipt {
    /// Adapter-owned correlation for the eventual [`ChildControlOutcome`].
    pub(crate) token: String,
}

/// Typed outcome of guiding one native child through its parent's managed writer.
///
/// Guidance is a steering user message delivered into the child's own native session by the
/// parent's RPC process; it never creates a child writer. Only a running or pending child can
/// accept guidance (verified OMP `steer_subagent` semantics).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum ChildStopOutcome {
    /// The native runtime accepted the stop (`cancelled: true`). Termination is observed separately.
    Cancelled,
    /// Nothing was cancelled because the child was not running (`cancelled: false`). Never success.
    NotRunning,
}

/// Typed final outcome of one dispatched child control, correlated by its receipt token.
///
/// This is the deferred half of the parent-owned child control path: the adapter publishes it
/// through the ordinary event stream once the native runtime answers, so approvals, polls and
/// other lane messages stay deliverable while the answer is outstanding.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum ChildControlOutcome {
    /// The native runtime answered the guidance command.
    Guide(Result<ChildGuideOutcome, ChildControlFailure>),
    /// The native runtime answered the stop command.
    Stop(Result<ChildStopOutcome, ChildControlFailure>),
}

/// Why a dispatched child control produced no typed disposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum ChildControlFailure {
    /// The provider refused the command; delivery is known to have failed and is never retried.
    Rejected,
    /// The native answer never arrived; delivery stays uncertain and is never resubmitted.
    Uncertain,
}
