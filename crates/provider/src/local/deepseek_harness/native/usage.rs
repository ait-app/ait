//! Native context occupancy is independent of cumulative billing counters.
use crate::{ports::agent_session::AgentSessionError, protocol::usage::AgentUsage};
use serde_json::Value;

pub(super) fn context(value: &Value) -> Result<Option<AgentUsage>, AgentSessionError> {
    let Some(used) = value["projectedTokens"]
        .as_u64()
        .or_else(|| value["pressureTokens"].as_u64())
    else {
        return Ok(None);
    };
    let usage = AgentUsage {
        context_window_used_tokens: Some(used),
        context_window_max_tokens: value["contextWindow"].as_u64(),
        ..AgentUsage::default()
    };
    if !usage.is_valid() {
        return Err(AgentSessionError::Failed);
    }
    Ok(Some(usage))
}
