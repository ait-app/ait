//! Discovery replies are tracked independently of the physical WebSocket reader.

/// Client methods implemented by this component; consumed by capability discovery.
pub(crate) const METHODS: &[MethodSpec] = &[
    MethodSpec::request("provider.usage.list.request"),
    MethodSpec::request("provider.available.list.request"),
    MethodSpec::request("provider.models.list.request"),
    MethodSpec::request("provider.modes.list.request"),
    MethodSpec::request("provider.features.list.request"),
    MethodSpec::request("provider.snapshot.get.request"),
    MethodSpec::request("provider.snapshot.refresh.request"),
];

use model::Context;
use model::methods::MethodSpec;
use model::outbound::QueueError;

use super::State;
use crate::service::agent_execution::AgentExecution;

/// Admit a discovery read on the executor's discovery lane and deliver it from a tracked task.
/// # Errors
/// Returns delivery failure; admission and execution errors use the original request ID.
pub(super) fn request(context: Context<'_>, state: &State) -> Result<(), QueueError> {
    super::agent_execution::respond_admitted(context, state, AgentExecution::admit_discovery, ())
}
