//! Read-only facts for one Agent; account quotas and child usage are separate resources.

use serde::Serialize;

use super::agent_lifecycle::AgentStatus;
use super::usage::AgentUsage;

/// Latest committed facts, without claiming a provider snapshot is a session total.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentSessionStatistics {
    /// Selected Agent identity, not its parent or children.
    pub(crate) agent_id: String,
    /// Provider responsible for these observations.
    pub(crate) provider: String,
    /// Last committed execution state.
    pub(crate) status: AgentStatus,
    /// Requested model, which can differ from the active native model.
    pub(crate) configured_model: Option<String>,
    /// Model confirmed by the native runtime; never inferred from configuration.
    pub(crate) effective_model: Option<String>,
    /// Runtime record update time, not a claimed metric observation timestamp.
    pub(crate) updated_at: String,
    /// The provider determines each counter's scope; consumers must not sum snapshots.
    pub(crate) usage_scope: UsageScope,
    /// Latest native facts, with explicit nulls for unavailable metrics.
    pub(crate) usage: StatisticsUsage,
}

/// Accounting semantics supported by the existing usage observation contract.
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UsageScope {
    /// Latest report; neither a per-turn delta nor a guaranteed session aggregate.
    ProviderSnapshot,
}

/// Nullable statistics view of the existing `AgentUsage` snapshot.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StatisticsUsage {
    pub(crate) input_tokens: Option<u64>,
    pub(crate) cached_input_tokens: Option<u64>,
    pub(crate) output_tokens: Option<u64>,
    pub(crate) total_cost_usd: Option<f64>,
    pub(crate) context_window_max_tokens: Option<u64>,
    pub(crate) context_window_used_tokens: Option<u64>,
}

impl From<AgentUsage> for StatisticsUsage {
    fn from(usage: AgentUsage) -> Self {
        Self {
            input_tokens: usage.input_tokens,
            cached_input_tokens: usage.cached_input_tokens,
            output_tokens: usage.output_tokens,
            total_cost_usd: usage.total_cost_usd,
            context_window_max_tokens: usage.context_window_max_tokens,
            context_window_used_tokens: usage.context_window_used_tokens,
        }
    }
}
