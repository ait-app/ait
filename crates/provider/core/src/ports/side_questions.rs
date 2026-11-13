//! Typed BTW side-question port (shared contract).
//!
//! Provider adapters implement auxiliary side-question execution behind this
//! port; the daemon side store owns topic identity, receipts, one-active
//! enforcement and the bounded projection. There is no string-action gateway:
//! native JSON parsing stays inside each adapter.
//!
//! Uniform contract for every implementation:
//! - A new topic freezes [`SideContextSnapshot`] once; follow-ups reuse it plus
//!   accumulated Q/A and never recapture main-conversation context.
//! - One active turn per Agent; a second ask while running rejects with
//!   [`AgentSessionError::Busy`].
//! - Acceptance is durable: an accepted ask is persisted by the runner before
//!   the ack; transport failure after send is uncertain, never auto-retried and
//!   never falls back to another execution.
//! - Cancellation never reports success unless the runner confirmed it.

use std::path::PathBuf;

use crate::ports::agent_session::AgentSessionError;
use crate::protocol::side_questions::{
    SideContextSnapshot, SideQuestionsState, SideStrategy, SideTopic,
};

/// Launch context for an auxiliary runner, never a second main-session writer.
#[derive(Debug, Clone)]
pub struct SideRunnerSpec {
    /// Exact selected provider/model, cwd and resolved sandbox confinement.
    pub session: crate::ports::agent_session::AgentSessionSpec,
    /// Ait-owned auxiliary storage, distinct from the main native session.
    pub directory: PathBuf,
    /// Ephemeral native launch overrides inherited from the source session, never persisted.
    pub environment: crate::ports::environment::AgentEnvironment,
}

/// Provider-independent inputs for one auxiliary side-question ask.
#[derive(Debug, Clone)]
pub struct SideAsk {
    /// Client-generated idempotency key; replays never re-execute.
    pub request_id: String,
    /// Nonempty question text.
    pub question: String,
    /// Existing topic id for a follow-up; `None` starts a new topic.
    pub record_id: Option<String>,
    /// Frozen display context; authoritative for new topics, ignored by
    /// follow-ups (the runner keeps the topic's original snapshot).
    pub snapshot: SideContextSnapshot,
    /// Persisted prior topic for stateless follow-up reconstruction.
    pub previous: Option<SideTopic>,
}

/// Durable acceptance outcome of one ask.
#[derive(Debug, Clone)]
pub struct SideAskReceipt {
    /// Strategy frozen for this topic.
    pub strategy: SideStrategy,
    /// Native-compatible accepted record, including a newly running follow-up.
    pub topic: SideTopic,
}

/// Confirmed cancellation outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SideCancelOutcome {
    /// Whether a running turn was cancelled; false when none matched.
    pub cancelled: bool,
}

/// Runtime-gated side-question capability.
///
/// Probe failure only removes this capability; it never erases baseline
/// provider features.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideCapability {
    /// Whether side questions can run on this host right now.
    pub available: bool,
    /// Stable human-readable reason when unavailable.
    pub reason: Option<String>,
}

impl SideCapability {
    /// Capability present without a reason.
    #[must_use]
    pub const fn available() -> Self {
        Self {
            available: true,
            reason: None,
        }
    }

    /// Capability absent with a stable reason.
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            available: false,
            reason: Some(reason.into()),
        }
    }
}

/// Auxiliary side-question execution owned by one provider adapter.
///
/// Implementations own their auxiliary processes and native storage; the
/// daemon owns receipts, one-active guards and projection bounds. Default
/// methods keep every non-BTW provider compiling and behaving identically.
#[async_trait::async_trait]
pub trait SideQuestions: std::fmt::Debug + Send + Sync {
    /// Strategy this adapter freezes for new topics.
    fn side_strategy(&self) -> SideStrategy;

    /// Runtime-gated capability; probing must not call a model.
    fn side_capability(&self) -> SideCapability {
        SideCapability::unavailable("side questions are not supported by this provider")
    }

    /// Stable reason for the most recent pre-execution rejection, when more specific than validation.
    /// Adapters reset this before validating each new ask; a context budget uses `context_limit`.
    fn side_rejection_reason(&self) -> Option<&str> {
        None
    }

    /// Start a new topic or follow up an existing one.
    ///
    /// This future is cancellation-safe: before the first native send, the adapter retains
    /// process ownership in the runner or a drop guard. A host deadline drops the future and
    /// records uncertain acceptance; later cancellation/reconciliation must remain possible.
    ///
    /// # Errors
    /// [`AgentSessionError::Busy`] while another turn runs;
    /// [`AgentSessionError::Rejected`] for pre-execution validation failures;
    /// [`AgentSessionError::Failed`] when acceptance is uncertain after send.
    async fn side_ask(&mut self, ask: SideAsk) -> Result<SideAskReceipt, AgentSessionError>;

    /// Cancel the running turn of a topic, or the running topic when absent.
    ///
    /// # Errors
    /// Transport failures; an uncertain cancellation is never reported successful.
    async fn side_cancel(
        &mut self,
        record_id: Option<&str>,
    ) -> Result<SideCancelOutcome, AgentSessionError>;

    /// Read durable topics, newest first, without starting inference.
    ///
    /// # Errors
    /// Native history read failures.
    async fn side_history(&mut self) -> Result<Vec<SideTopic>, AgentSessionError>;

    /// Reconcile one uncertain request without sending inference again.
    ///
    /// `Some(topic)` identifies the request's authoritative native record.
    /// `None` confirms that no execution remains but no record can be recovered.
    /// # Errors
    /// Returns an error while ownership or acceptance is still unknown.
    async fn reconcile_side(
        &mut self,
        _request_id: &str,
    ) -> Result<Option<SideTopic>, AgentSessionError> {
        Err(AgentSessionError::Unavailable)
    }

    /// Bounded live projection for `providerUi.sideQuestions`.
    fn side_projection(&self) -> SideQuestionsState;

    /// Drain one incremental projection update, when the runner observed new
    /// native frames since the last poll. Returns `None` when nothing changed.
    fn poll_side(&mut self) -> Option<SideQuestionsState> {
        None
    }

    /// Release auxiliary processes and temporary artifacts; durable topic
    /// history remains readable afterwards.
    async fn close_side(&mut self) -> Result<(), AgentSessionError> {
        Ok(())
    }
}
