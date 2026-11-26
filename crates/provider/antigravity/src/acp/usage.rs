//! ACP prompt totals are per turn; `usage_update` only describes context occupancy.

use domain::agent_runtime::AgentPersistenceHandle;
use provider_core::{
    ports::{
        agent_session::AgentSessionError,
        usage_ledger::{RequestUsage, UsageBoundary},
    },
    protocol::usage::AgentUsage,
};
use serde_json::Value;

/// Restore only the accumulated ACP consumption; context occupancy is an independent snapshot.
pub(super) fn restore(
    handle: Option<&AgentPersistenceHandle>,
) -> Result<AgentUsage, AgentSessionError> {
    let Some(value) = handle.and_then(|handle| handle.metadata.as_ref()?.get("acpUsage")) else {
        return Ok(AgentUsage::default());
    };
    let usage: AgentUsage =
        serde_json::from_value(value.clone()).map_err(|_| AgentSessionError::Rejected)?;
    if !usage.is_valid()
        || usage.context_window_used_tokens.is_some()
        || usage.context_window_max_tokens.is_some()
        || usage.total_cost_usd.is_some()
    {
        return Err(AgentSessionError::Rejected);
    }
    Ok(usage)
}

/// Add each correlated per-turn result once and publish a cumulative session snapshot.
pub(super) fn accumulate(
    usage: &mut AgentUsage,
    request: &RequestUsage,
) -> Result<(), AgentSessionError> {
    let sum = |value: Option<u64>, increment: u64| {
        value
            .unwrap_or_default()
            .checked_add(increment)
            .filter(|value| *value <= 9_007_199_254_740_991)
            .ok_or(AgentSessionError::Failed)
    };
    let updated = AgentUsage {
        input_tokens: Some(sum(usage.input_tokens, request.input_tokens)?),
        cached_input_tokens: Some(sum(usage.cached_input_tokens, request.cache_read_tokens)?),
        output_tokens: Some(sum(usage.output_tokens, request.output_tokens)?),
        ..AgentUsage::default()
    };
    *usage = updated;
    Ok(())
}

/// Decode the native disjoint token buckets without treating context occupancy as consumption.
pub(super) fn request(
    turn: &str,
    usage: &Value,
) -> Result<Option<RequestUsage>, AgentSessionError> {
    if usage.is_null() {
        return Ok(None);
    }
    if !usage.is_object() {
        return Err(AgentSessionError::Failed);
    }
    let counter = |field| -> Result<Option<u64>, AgentSessionError> {
        usage
            .get(field)
            .map(|value| value.as_u64().ok_or(AgentSessionError::Failed))
            .transpose()
    };
    let input = counter("inputTokens")?;
    let output = counter("outputTokens")?;
    let read = counter("cachedReadTokens")?;
    let write = counter("cachedWriteTokens")?;
    let request = RequestUsage {
        segment_id: format!("antigravity:{turn}"),
        parent: None,
        model: None,
        boundary: UsageBoundary::Turn,
        input_tokens: input.unwrap_or_default(),
        output_tokens: output.unwrap_or_default(),
        cache_read_tokens: read.unwrap_or_default(),
        cache_write_tokens: write.unwrap_or_default(),
        cache_write_1h_tokens: None,
        reasoning_tokens: counter("thoughtTokens")?,
        cost_usd_reported: None,
        complete: input.is_some() && output.is_some() && read.is_some() && write.is_some(),
    };
    if !request.is_valid() {
        return Err(AgentSessionError::Failed);
    }
    Ok(Some(request))
}
