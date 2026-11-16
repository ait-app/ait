//! Canonical BTW side-question contract (shared contract).
//!
//! One uniform contract for every provider auxiliary runner: a topic freezes the
//! display context already persisted at Submit, then accumulates question/answer
//! turns. Follow-ups reuse the frozen snapshot plus prior turns; they never
//! recapture main-conversation context. The daemon side store owns topic
//! identity, receipts and the bounded projection; provider adapters own only
//! native execution behind the typed [`crate::ports::side_questions`] port.
//!
//! Wire shapes here are mirrored by `packages/protocol` (zod) and consumed by
//! `agent.side_question.{ask,cancel,history}` plus the optional
//! `AgentSnapshotPayload.providerUi.sideQuestions` projection.

use serde::{Deserialize, Serialize};

/// Maximum topics retained in one bounded projection.
pub const MAX_TOPICS: usize = 8;
/// Maximum turns shown per topic in a live projection; durable history remains complete.
pub const MAX_TURNS: usize = 8;
/// Hard byte budget for one projected topic record.
pub const MAX_RECORD_BYTES: usize = 64 * 1024;
/// Hard byte budget for one question text.
pub const MAX_QUESTION_BYTES: usize = 4 * 1024;
/// Hard byte budget for one answer text.
pub const MAX_ANSWER_BYTES: usize = 48 * 1024;
/// Hard byte budget for one redacted error text.
pub const MAX_ERROR_BYTES: usize = 4 * 1024;
/// Hard byte budget for the frozen display-context snapshot transcript.
pub const MAX_SNAPSHOT_BYTES: usize = 64 * 1024;

/// Lifecycle of one side-question turn.
///
/// `interrupted` marks a turn whose auxiliary process died while it ran;
/// reconciliation settles it without resubmission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideTurnStatus {
    /// The turn was accepted and is producing an answer.
    Running,
    /// The answer completed.
    Complete,
    /// The turn was cancelled before completion.
    Cancelled,
    /// The turn failed with a redacted error.
    Error,
    /// The auxiliary process died while the turn ran.
    Interrupted,
}

impl SideTurnStatus {
    /// Whether the turn can no longer change.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Running)
    }
}

/// Execution strategy chosen once per topic at Submit; never re-chosen mid-run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SideStrategy {
    /// Provider-native side-conversation execution in an independent auxiliary session.
    Native,
    /// Provider-local isolated read-only execution with the same shared lifecycle.
    Isolated,
}

/// Durable acceptance receipt for one ask, mirroring the main-input discipline.
///
/// `Uncertain` never auto-retries and never falls back to another execution;
/// recovery reconciles native state and then settles the receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideReceiptState {
    /// The ask was persisted and accepted by the auxiliary runner.
    Accepted,
    /// The ask was rejected before any execution (validation, busy, unsupported).
    Rejected,
    /// Acceptance is unknown after a transport failure; reconcile before settling.
    Uncertain,
}

/// Native image content retained losslessly in BTW history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BtwImage {
    /// Native content discriminator (`image`).
    pub r#type: String,
    /// Base64-encoded image bytes.
    pub data: String,
    /// Native image media type.
    pub mime_type: String,
}

/// One native question/answer turn inside a topic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BtwHistoryTurn {
    /// User question text, bounded to [`MAX_QUESTION_BYTES`].
    pub question: String,
    /// Native images, when present; not recaptured from the main session.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<BtwImage>,
    /// Accumulated answer text, bounded to [`MAX_ANSWER_BYTES`].
    pub answer: String,
    /// Turn lifecycle.
    pub status: SideTurnStatus,
    /// Creation time, milliseconds since the Unix epoch.
    pub created_at: u64,
    /// Last update time, milliseconds since the Unix epoch.
    pub updated_at: u64,
    /// Redacted failure text, when the turn errored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Question/answer history shared by native and isolated provider runners.
///
/// Root status is not the latest status when follow-ups exist.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BtwHistoryRecord {
    /// Provider-native topic identity (OMP record id or daemon-generated id).
    pub id: String,
    /// First-turn question, bounded to [`MAX_QUESTION_BYTES`].
    pub question: String,
    /// Native first-turn images.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<BtwImage>,
    /// First-turn answer, bounded to [`MAX_ANSWER_BYTES`].
    pub answer: String,
    /// First-turn lifecycle.
    pub status: SideTurnStatus,
    /// Creation time, milliseconds since the Unix epoch.
    pub created_at: u64,
    /// Last update time, milliseconds since the Unix epoch.
    pub updated_at: u64,
    /// First-turn redacted failure text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Native leaf identity captured at topic creation, when available.
    pub leaf_id: Option<String>,
    /// Optional native topic title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Follow-up turns in order; live projection bounds do not limit durable history.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub follow_ups: Vec<BtwHistoryTurn>,
}

/// Ait topic projection; native record fields remain at the wire root.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SideTopic {
    /// Canonical native-compatible record.
    #[serde(flatten)]
    pub record: BtwHistoryRecord,
    /// Strategy frozen for this topic at Submit.
    pub strategy: SideStrategy,
    /// Whether any retained text was clipped by the projection bounds.
    pub truncated: bool,
}

impl BtwHistoryRecord {
    /// The latest turn: last follow-up, else the root turn.
    #[must_use]
    pub fn latest_turn(&self) -> SideTurnRef<'_> {
        match self.follow_ups.last() {
            Some(turn) => SideTurnRef {
                question: &turn.question,
                answer: &turn.answer,
                status: turn.status,
                created_at: turn.created_at,
                updated_at: turn.updated_at,
                error: turn.error.as_deref(),
            },
            None => SideTurnRef {
                question: &self.question,
                answer: &self.answer,
                status: self.status,
                created_at: self.created_at,
                updated_at: self.updated_at,
                error: self.error.as_deref(),
            },
        }
    }

    /// Total turn count including the root turn.
    #[must_use]
    pub fn turn_count(&self) -> usize {
        1 + self.follow_ups.len()
    }
}

impl SideTopic {
    /// Latest turn, independent of the root's possibly stale status.
    #[must_use]
    pub fn latest_turn(&self) -> SideTurnRef<'_> {
        self.record.latest_turn()
    }
}

/// Borrowed view of the latest turn, avoiding clones in guards and projections.
#[derive(Debug, Clone, Copy)]
pub struct SideTurnRef<'a> {
    /// Question text.
    pub question: &'a str,
    /// Answer text.
    pub answer: &'a str,
    /// Lifecycle.
    pub status: SideTurnStatus,
    /// Creation time, milliseconds since the Unix epoch.
    pub created_at: u64,
    /// Last update time, milliseconds since the Unix epoch.
    pub updated_at: u64,
    /// Redacted failure text.
    pub error: Option<&'a str>,
}

/// One readable display-context entry frozen into a topic snapshot.
///
/// Tool activity is reduced to its displayed summary, never raw payloads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SideContextEntry {
    /// Display role: `user`, `assistant` or `tool`.
    pub role: String,
    /// Readable display text, already redacted.
    pub text: String,
}

/// Frozen display context captured from the already-persisted main projection.
///
/// Written once per topic at the first ask; follow-ups and transfer reuse it
/// verbatim. Versioned so promotion can evolve without breaking stores.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SideContextSnapshot {
    /// Schema version; currently 1.
    pub v: u32,
    /// Owning Agent identity.
    pub agent_id: String,
    /// Provider identity frozen with the topic.
    pub provider: String,
    /// Selected model frozen with the topic; never a summarizer route.
    pub model: Option<String>,
    /// Native session identity at capture, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_session_id: Option<String>,
    /// Native leaf identity at capture, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_leaf_id: Option<String>,
    /// Capture time, milliseconds since the Unix epoch.
    pub captured_at: u64,
    /// Bounded readable transcript, oldest first.
    pub entries: Vec<SideContextEntry>,
    /// Whether the transcript was clipped at [`MAX_SNAPSHOT_BYTES`].
    pub truncated: bool,
}

/// Bounded side-question projection pushed inside `providerUi.sideQuestions`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SideQuestionsState {
    /// Whether an earlier send still has unresolved acceptance; clients may refresh but never resend it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_acceptance: Option<bool>,
    /// Runtime capability result after a no-inference probe, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available: Option<bool>,
    /// Specific runtime reason when this provider cannot run side questions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
    /// Retained topics, newest first, at most [`MAX_TOPICS`].
    pub records: Vec<SideTopic>,
    /// Panel-scoped redacted error, isolated from the main conversation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Number of topics dropped by the projection bound.
    pub omitted: usize,
    /// Topic id to reveal after a live start or history restore.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reveal: Option<String>,
    /// Topic observed running on this connection, never a stale disk record.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub running_id: Option<String>,
}

/// Optional provider-UI state shared by supported adapters.
///
/// Additive and optional: old clients ignore it; later provider features extend
/// this same surface rather than introducing a second one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderUiState {
    /// BTW side-question projection, when the provider supports it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side_questions: Option<SideQuestionsState>,
}

/// `agent.side_question.ask.request` payload.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SideAskRequest {
    /// Registered Agent identifier.
    pub agent_id: String,
    /// Client-generated idempotency key; replays never re-execute.
    pub request_id: String,
    /// Nonempty question text.
    pub question: String,
    /// Existing topic id for a follow-up; absent starts a new topic.
    #[serde(default)]
    pub record_id: Option<String>,
}

/// `agent.side_question.ask.response` payload.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SideAskResponse {
    /// Durable acceptance receipt for this request id.
    pub receipt: SideReceiptState,
    /// Topic identity once accepted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_id: Option<String>,
    /// Strategy frozen for the topic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy: Option<SideStrategy>,
    /// Stable rejection reason, when rejected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `agent.side_question.cancel.request` payload.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SideCancelRequest {
    /// Registered Agent identifier.
    pub agent_id: String,
    /// Topic to cancel; absent cancels the running topic.
    #[serde(default)]
    pub record_id: Option<String>,
}

/// `agent.side_question.cancel.response` payload.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SideCancelResponse {
    /// Whether a running turn was cancelled; false when none matched.
    pub cancelled: bool,
}

/// `agent.side_question.history.request` payload.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SideHistoryRequest {
    /// Registered Agent identifier.
    pub agent_id: String,
}

/// Read-only promotion context for a floating BTW conversation.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SideTransferRequest {
    /// Registered source Agent identifier.
    pub agent_id: String,
    /// Topic whose frozen context and full Q/A are promoted.
    pub record_id: String,
}

/// Promotion bundle; creating this value never sends inference or mutates a session.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SideTransferResponse {
    /// Original fixed context, including selected provider/model identity.
    pub snapshot: SideContextSnapshot,
    /// The topic and every retained follow-up, in submission order.
    pub topic: SideTopic,
}

/// Stable rejection reasons surfaced by the ask RPC.
pub mod reason {
    /// Another side question is still running for this Agent.
    pub const BUSY: &str = "busy";
    /// The topic's latest turn is not complete; follow-up is not allowed yet.
    pub const TOPIC_NOT_SETTLED: &str = "topic_not_settled";
    /// The follow-up references an unknown topic.
    pub const UNKNOWN_TOPIC: &str = "unknown_topic";
    /// The provider does not support side questions on this host.
    pub const UNSUPPORTED_PROVIDER: &str = "unsupported_provider";
    /// The question text is empty or exceeds the bound.
    pub const INVALID_QUESTION: &str = "invalid_question";
    /// The provider cannot fit another side turn; promoting preserves the full retained history.
    pub const CONTEXT_LIMIT: &str = "context_limit";
}

/// Clip text to a byte budget on a UTF-8 boundary; returns the clip and flag.
#[must_use]
pub fn bounded(text: &str, limit: usize) -> (&str, bool) {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], end < text.len())
}

/// Measure the wire byte size without allocating a serialized copy.
/// # Errors
/// Returns serialization failures for an unsupported value.
pub fn encoded_size(value: &impl Serialize) -> Result<usize, serde_json::Error> {
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut count = Count(0);
    serde_json::to_writer(&mut count, value)?;
    Ok(count.0)
}

#[cfg(test)]
mod tests;
