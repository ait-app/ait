//! Prepare private launch settings while retaining the user's durable configuration.

use std::borrow::Cow;

use domain::agent_runtime::PersistedAgentRuntimeRecord;

use super::{AgentManager, AgentManagerError};
use crate::ports::agent_session::{AgentResumePurpose, AgentSessionSpec};
use crate::ports::mcp_scope::McpSubject;
use crate::service::agent_mcp::Binding;

impl AgentManager {
    /// Bind the daemon MCP for an interactive session; guest-created Agents are stripped.
    ///
    /// The daemon MCP reaches every Workspace on this Host, so an Agent labelled with
    /// [`crate::service::agent_mcp::GUEST_SHARE_LABEL`] never receives it, and any `ait`
    /// entry it configured is removed. Bot and subtree sessions that cannot receive their
    /// scoped credential (history, or no HTTP MCP support) are stripped as well.
    pub(super) fn prepare_mcp<'a>(
        &self,
        subject: &McpSubject<'_>,
        spec: &'a AgentSessionSpec,
        purpose: AgentResumePurpose,
    ) -> Result<(Cow<'a, AgentSessionSpec>, Option<Binding>), AgentManagerError> {
        let binding = if subject
            .labels
            .contains_key(crate::service::agent_mcp::GUEST_SHARE_LABEL)
        {
            Some(Binding::Strip)
        } else {
            let deliverable = purpose == AgentResumePurpose::Interactive
                && self
                    .clients
                    .get(&spec.provider)
                    .is_some_and(|client| client.supports_agent_mcp());
            self.mcp
                .as_ref()
                .map(|mcp| mcp.bind(subject, &spec.config, deliverable))
                .transpose()
                .map_err(|_| AgentManagerError::Registry)?
                .flatten()
        };
        let mut launch = Cow::Borrowed(spec);
        if let Some(binding) = &binding {
            binding.apply(&mut launch.to_mut().config);
        }
        Ok((launch, binding))
    }

    /// Keep the binding of a session that opened, or revoke its credential if opening failed.
    pub(super) fn retain_mcp(
        &mut self,
        agent_id: &str,
        opened: Result<PersistedAgentRuntimeRecord, AgentManagerError>,
        binding: Option<Binding>,
    ) -> Result<PersistedAgentRuntimeRecord, AgentManagerError> {
        match opened {
            Ok(record) => {
                self.live
                    .get_mut(agent_id)
                    .expect("successful registration retains the native session")
                    .mcp = binding;
                Ok(record)
            }
            Err(error) => {
                if binding.as_ref().is_some_and(Binding::is_scoped) {
                    self.release_mcp(agent_id);
                }
                Err(error)
            }
        }
    }

    /// Forget a live session, revoking the scoped credential it was opened with.
    pub(super) fn drop_live(&mut self, agent_id: &str) {
        if self
            .live
            .remove(agent_id)
            .and_then(|agent| agent.mcp)
            .is_some_and(|binding| binding.is_scoped())
        {
            self.release_mcp(agent_id);
        }
    }

    fn release_mcp(&self, agent_id: &str) {
        if let Some(mcp) = &self.mcp {
            mcp.release(agent_id);
        }
    }
}
