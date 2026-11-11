//! Statistics read the committed runtime port, never a native session or timeline writer.

use domain::agent_runtime::PersistedAgentRuntimeRecord;
use serde::Deserialize;
use serde_json::Value;

use crate::protocol::agent_lifecycle::AgentIdRequest;
use crate::protocol::statistics::{AgentSessionStatistics, UsageScope};
use crate::protocol::usage::AgentUsage;
use crate::rpc::ErrorCode;
use crate::service::agent_runtime::{AgentRuntimeDirectory, resolved_updated_at};
use crate::support::rpc::encode;

/// Read the selected Agent's last committed facts, including observed and hidden sessions.
/// Returns identity or storage errors without restoring a provider or starting a turn.
pub(crate) fn get(
    directory: &AgentRuntimeDirectory,
    request: &AgentIdRequest,
) -> Result<Value, ErrorCode> {
    let resolved = directory
        .get_readable(&request.agent_id)
        .map_err(|error| super::map_error(&error))?;
    encode(project(&resolved.agent), ErrorCode::AgentIo)
}

fn project(record: &PersistedAgentRuntimeRecord) -> AgentSessionStatistics {
    AgentSessionStatistics {
        agent_id: record.id.clone(),
        provider: record.provider.clone(),
        status: super::protocol_status(record.last_status),
        configured_model: record
            .config
            .as_ref()
            .and_then(|config| config.model.clone()),
        effective_model: record
            .runtime_info
            .as_ref()
            .and_then(|info| info.model.clone()),
        updated_at: super::wire_timestamp(resolved_updated_at(record)),
        usage_scope: UsageScope::ProviderSnapshot,
        usage: last_usage(record).unwrap_or_default().into(),
    }
}

/// Decode only the established typed usage field; malformed snapshots are unavailable.
pub(crate) fn last_usage(record: &PersistedAgentRuntimeRecord) -> Option<AgentUsage> {
    record
        .runtime_info
        .as_ref()?
        .extra
        .as_ref()?
        .get("lastUsage")
        .and_then(|usage| AgentUsage::deserialize(usage).ok())
        .filter(AgentUsage::is_valid)
}

#[cfg(test)]
mod tests;
