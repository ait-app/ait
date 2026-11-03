//! Prepare private launch settings while retaining the user's durable configuration.

use std::borrow::Cow;
use std::collections::BTreeMap;

use domain::agent_runtime::PersistedAgentRuntimeRecord;

use super::{AgentManager, AgentManagerError};
use crate::ports::agent_session::{AgentResumePurpose, AgentSessionSpec};
use crate::service::agent_mcp::Binding;

impl AgentManager {
    /// Bind the daemon MCP for an interactive session, except for guest-created Agents.
    ///
    /// The daemon MCP reaches every Workspace on this Host, so an Agent labelled with
    /// [`crate::service::agent_mcp::GUEST_SHARE_LABEL`] never receives it.
    pub(super) fn prepare_mcp<'a>(
        &self,
        id: &str,
        spec: &'a AgentSessionSpec,
        purpose: AgentResumePurpose,
        labels: &BTreeMap<String, String>,
    ) -> Result<(Cow<'a, AgentSessionSpec>, Option<Binding>), AgentManagerError> {
        let supported = self
            .clients
            .get(&spec.provider)
            .is_some_and(|client| client.supports_agent_mcp());
        let guest = labels.contains_key(crate::service::agent_mcp::GUEST_SHARE_LABEL);
        let binding = if supported && !guest && purpose == AgentResumePurpose::Interactive {
            self.mcp
                .as_ref()
                .map(|mcp| mcp.bind(id, &spec.config))
                .transpose()
                .map_err(|_| AgentManagerError::Registry)?
                .flatten()
        } else {
            None
        };
        let mut launch = Cow::Borrowed(spec);
        if let Some(binding) = &binding {
            binding.apply(&mut launch.to_mut().config);
        }
        Ok((launch, binding))
    }

    pub(super) fn retain_mcp(
        &mut self,
        record: PersistedAgentRuntimeRecord,
        binding: Option<Binding>,
    ) -> PersistedAgentRuntimeRecord {
        self.live
            .get_mut(&record.id)
            .expect("successful registration retains the native session")
            .mcp = binding;
        record
    }
}
