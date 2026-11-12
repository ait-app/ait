//! Additive request usage that adapters report to the per-host harness usage ledger.
//!
//! Unlike the replaceable `AgentUsage` snapshot, request usage is summed.

use serde::{Deserialize, Serialize};

/// Largest counter that JSON clients represent exactly (2^53 - 1).
const MAX_EXACT_COUNTER: u64 = 9_007_199_254_740_991;

/// How the provider delimited the tokens in one [`RequestUsage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageBoundary {
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
    pub const fn as_str(self) -> &'static str {
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
pub struct RequestUsage {
    /// Stable identity of the native request, unique within an Agent; the idempotency key.
    /// It includes the provider and turn identity so replays map to the same segment.
    pub segment_id: String,
    /// Stream key of the native sub-agent that issued the request, if any.
    pub parent: Option<String>,
    /// Model the provider reported for this request, if any.
    pub model: Option<String>,
    /// How the provider delimited these tokens.
    pub boundary: UsageBoundary,
    /// Input tokens, excluding cache read and cache write tokens.
    pub input_tokens: u64,
    /// Output tokens, including reasoning tokens.
    pub output_tokens: u64,
    /// Input tokens served from the provider's prompt cache.
    pub cache_read_tokens: u64,
    /// Input tokens written to the provider's prompt cache.
    pub cache_write_tokens: u64,
    /// Subset of `cache_write_tokens` written with a one-hour lifetime; never added to totals.
    pub cache_write_1h_tokens: Option<u64>,
    /// Subset of `output_tokens` spent on reasoning; never added to totals.
    pub reasoning_tokens: Option<u64>,
    /// Cost in USD as reported by the provider itself; never estimated from prices.
    pub cost_usd_reported: Option<f64>,
    /// Whether the terminal native frame covered every token bucket.
    pub complete: bool,
}

impl RequestUsage {
    /// Whether this record is safe to store and sum.
    ///
    /// Rejects an empty `segment_id`, counters above 2^53 - 1, a negative or nonfinite cost,
    /// and diagnostic subsets larger than their parent bucket.
    #[must_use]
    pub fn is_valid(&self) -> bool {
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

#[cfg(test)]
mod tests;
