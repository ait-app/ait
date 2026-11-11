//! Per-host harness usage ledger: additive request usage grouped into provider calls.
//!
//! Unlike the replaceable `AgentUsage` snapshot, request usage is summed. Ledger writes never fail
//! or delay a turn: implementations queue each write and log failures with `tracing::warn!`.

use std::fmt::Debug;

use serde::{Deserialize, Serialize};

/// Largest counter that JSON clients represent exactly (2^53 - 1).
const MAX_EXACT_COUNTER: u64 = 9_007_199_254_740_991;

/// How the provider delimited the tokens in one [`RequestUsage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UsageBoundary {
    /// The provider reported usage for exactly one model request.
    Request,
    /// The provider reported one total for the whole turn.
    Turn,
    /// The usage is the difference between two cumulative provider counters.
    CumulativeDelta,
}

impl UsageBoundary {
    /// Stable `snake_case` name used in storage and on the wire.
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Turn => "turn",
            Self::CumulativeDelta => "cumulative_delta",
        }
    }
}

/// One model request inside a provider turn, in disjoint token buckets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RequestUsage {
    /// Stable identity of the native request, unique within an Agent; the idempotency key.
    /// It includes the provider and turn identity so replays map to the same segment.
    pub(crate) segment_id: String,
    /// Stream key of the native sub-agent that issued the request, if any.
    pub(crate) parent: Option<String>,
    /// Model the provider reported for this request, if any.
    pub(crate) model: Option<String>,
    /// How the provider delimited these tokens.
    pub(crate) boundary: UsageBoundary,
    /// Input tokens, excluding cache read and cache write tokens.
    pub(crate) input_tokens: u64,
    /// Output tokens, including reasoning tokens.
    pub(crate) output_tokens: u64,
    /// Input tokens served from the provider's prompt cache.
    pub(crate) cache_read_tokens: u64,
    /// Input tokens written to the provider's prompt cache.
    pub(crate) cache_write_tokens: u64,
    /// Subset of `cache_write_tokens` written with a one-hour lifetime; never added to totals.
    pub(crate) cache_write_1h_tokens: Option<u64>,
    /// Subset of `output_tokens` spent on reasoning; never added to totals.
    pub(crate) reasoning_tokens: Option<u64>,
    /// Cost in USD as reported by the provider itself; never estimated from prices.
    pub(crate) cost_usd_reported: Option<f64>,
    /// Whether the terminal native frame covered every token bucket.
    pub(crate) complete: bool,
}

impl RequestUsage {
    /// Whether this record is safe to store and sum.
    ///
    /// Rejects an empty `segment_id`, counters above 2^53 - 1, a negative or nonfinite cost,
    /// and diagnostic subsets larger than their parent bucket.
    #[must_use]
    pub(crate) fn is_valid(&self) -> bool {
        !self.segment_id.is_empty()
            && [
                Some(self.input_tokens),
                Some(self.output_tokens),
                Some(self.cache_read_tokens),
                Some(self.cache_write_tokens),
                self.cache_write_1h_tokens,
                self.reasoning_tokens,
            ]
            .into_iter()
            .flatten()
            .all(|tokens| tokens <= MAX_EXACT_COUNTER)
            && self
                .cache_write_1h_tokens
                .is_none_or(|tokens| tokens <= self.cache_write_tokens)
            && self
                .reasoning_tokens
                .is_none_or(|tokens| tokens <= self.output_tokens)
            && self
                .cost_usd_reported
                .is_none_or(|cost| cost.is_finite() && cost >= 0.0)
    }
}

/// What started a provider call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CallSource {
    /// Input from a user through RPC, voice, queued delivery or steer-when-idle.
    User,
    /// Input delivered to a Bot's internal session.
    Bot,
    /// Input delivered by a schedule.
    Schedule,
    /// Input delivered by a heartbeat.
    Heartbeat,
    /// A native turn that the provider started without host input, such as a Codex goal.
    Autonomous,
    /// An auxiliary summary or title generation request.
    Summary,
}

impl CallSource {
    /// Stable `snake_case` name used in storage and on the wire.
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Bot => "bot",
            Self::Schedule => "schedule",
            Self::Heartbeat => "heartbeat",
            Self::Autonomous => "autonomous",
            Self::Summary => "summary",
        }
    }
}

/// Lifecycle state of a provider call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CallOutcome {
    /// The call started and has not finished yet.
    Running,
    /// The native provider drained the turn successfully.
    Completed,
    /// The native provider failed or exited.
    Failed,
    /// The native provider acknowledged interruption.
    Cancelled,
    /// The provider refused to start the turn; the call has no requests.
    Rejected,
    /// The host stopped while the call was running.
    Interrupted,
}

impl CallOutcome {
    /// Stable `snake_case` name used in storage and on the wire.
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Rejected => "rejected",
            Self::Interrupted => "interrupted",
        }
    }
}

/// How completely a finished call's requests account for its tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UsageStatus {
    /// A completed call with at least one request, each with a request boundary and complete
    /// buckets. A failed, cancelled or interrupted call is at most [`Self::LowerBound`].
    Exact,
    /// Some tokens were recorded, but the totals may undercount.
    LowerBound,
    /// No requests were recorded, or the provider does not report token usage.
    Missing,
}

impl UsageStatus {
    /// Stable `snake_case` name used in storage and on the wire.
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::LowerBound => "lower_bound",
            Self::Missing => "missing",
        }
    }
}

/// Start of one provider call, recorded as [`CallOutcome::Running`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CallBegin {
    /// Host-generated identity of the call.
    pub(crate) call_id: String,
    /// Agent that owns the call; empty for a summary call, which belongs to no Agent.
    pub(crate) agent_id: String,
    /// Provider that serves the call.
    pub(crate) provider: String,
    /// Configured or runtime model when the call started, if known.
    pub(crate) model: Option<String>,
    /// What started the call.
    pub(crate) source: CallSource,
    /// Workspace of the owning Agent, if any.
    pub(crate) workspace_id: Option<String>,
    /// Start time in Unix milliseconds (UTC).
    pub(crate) started_at: i64,
}

/// One request attributed to a running or finished call.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RequestRecord {
    /// Call that the request belongs to.
    pub(crate) call_id: String,
    /// Agent that owns the call; scopes `usage.segment_id`.
    pub(crate) agent_id: String,
    /// Observation time in Unix milliseconds (UTC).
    pub(crate) recorded_at: i64,
    /// Provider-reported request usage.
    pub(crate) usage: RequestUsage,
}

/// Terminal state of one provider call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CallFinish {
    /// Call to finish.
    pub(crate) call_id: String,
    /// Terminal outcome; never [`CallOutcome::Running`].
    pub(crate) outcome: CallOutcome,
    /// End time in Unix milliseconds (UTC).
    pub(crate) ended_at: i64,
    /// Whether the session reports token usage; `false` makes the call [`UsageStatus::Missing`].
    pub(crate) token_usage_available: bool,
}

/// One dimension that summary rows are grouped by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UsageGroup {
    /// Group by provider identity.
    Provider,
    /// Group by attributed model.
    Model,
    /// Group by call source.
    Source,
    /// Group by call outcome.
    Outcome,
    /// Group by UTC calendar day.
    Day,
}

/// Time range and grouping of a ledger summary; the `provider.usage.ledger` request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UsageQuery {
    /// Inclusive range start in Unix milliseconds (UTC).
    pub(crate) from: i64,
    /// Exclusive range end in Unix milliseconds (UTC).
    pub(crate) to: i64,
    /// Dimensions that rows are grouped by; empty yields one total row.
    pub(crate) group_by: Vec<UsageGroup>,
}

/// Group identity of one summary row; only the queried dimensions are set.
///
/// On the wire, dimensions that were not queried are omitted rather than sent as `null`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageKey {
    /// Provider identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) provider: Option<String>,
    /// Attributed model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) model: Option<String>,
    /// Call source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source: Option<CallSource>,
    /// Call outcome.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) outcome: Option<CallOutcome>,
    /// UTC calendar day as `YYYY-MM-DD`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) day: Option<String>,
}

/// Totals of one summary group.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageRow {
    /// Group identity.
    pub(crate) key: UsageKey,
    /// Provider calls.
    pub(crate) calls: u64,
    /// Calls that ended as failed.
    pub(crate) failed_calls: u64,
    /// Recorded requests.
    pub(crate) requests: u64,
    /// Input tokens, excluding cache read and cache write tokens.
    pub(crate) input_tokens: u64,
    /// Output tokens, including reasoning tokens.
    pub(crate) output_tokens: u64,
    /// Cache read tokens.
    pub(crate) cache_read_tokens: u64,
    /// Cache write tokens.
    pub(crate) cache_write_tokens: u64,
    /// Sum of provider-reported costs in USD.
    pub(crate) reported_cost_usd: f64,
    /// Requests that carried a provider-reported cost.
    pub(crate) cost_reported_requests: u64,
    /// Calls whose token totals may undercount.
    pub(crate) lower_bound_calls: u64,
    /// Calls without token usage.
    pub(crate) missing_calls: u64,
}

/// Grouped ledger totals for one query; the `provider.usage.ledger` response.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageSummary {
    /// One row per group.
    pub(crate) rows: Vec<UsageRow>,
    /// When this host's ledger started, in Unix milliseconds (UTC); no history predates it.
    pub(crate) since: i64,
}

/// Ledger read failure; writes never report errors to callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum UsageLedgerError {
    /// This host has no ledger, or it is shutting down.
    #[error("usage ledger is unavailable")]
    Unavailable,
    /// The ledger could not be read.
    #[error("usage ledger read failed")]
    Failed,
}

/// Append-only per-host record of provider calls and their request usage.
///
/// Write methods return immediately: implementations queue the work and log failures with
/// `tracing::warn!`, so a ledger never fails or delays a turn.
#[async_trait::async_trait]
pub(crate) trait UsageLedger: Debug + Send + Sync {
    /// Queue a new call in the running state.
    fn begin_call(&self, call: CallBegin);

    /// Queue one request; a repeated `(agent_id, segment_id)` is ignored.
    fn record_request(&self, request: RequestRecord);

    /// Queue the terminal outcome of a call and derive its [`UsageStatus`].
    fn finish_call(&self, finish: CallFinish);

    /// Queue marking every call still running as interrupted, as of `ended_at` (Unix ms).
    fn recover_interrupted(&self, ended_at: i64);

    /// Summarize the calls and requests within the query range.
    /// # Errors
    /// Returns unavailable without a ledger, or a failed storage read.
    async fn summarize(&self, query: UsageQuery) -> Result<UsageSummary, UsageLedgerError>;
}

/// Ledger for tests and hosts without usage storage: discards writes, cannot summarize.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NoopUsageLedger;

#[async_trait::async_trait]
impl UsageLedger for NoopUsageLedger {
    fn begin_call(&self, _call: CallBegin) {}

    fn record_request(&self, _request: RequestRecord) {}

    fn finish_call(&self, _finish: CallFinish) {}

    fn recover_interrupted(&self, _ended_at: i64) {}

    async fn summarize(&self, _query: UsageQuery) -> Result<UsageSummary, UsageLedgerError> {
        Err(UsageLedgerError::Unavailable)
    }
}

#[cfg(test)]
mod tests;
