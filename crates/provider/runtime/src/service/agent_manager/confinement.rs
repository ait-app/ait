//! Attach OS write confinement to native launches placed in a sandboxed Workspace.

use std::borrow::Cow;

use domain::workspace::sandbox::WriteConfinement;
use model::ErrorCode;

use super::{AgentManager, AgentManagerError};
use provider_core::ports::agent_session::AgentSessionSpec;

impl AgentManager {
    /// Attach the confinement required by `workspace_id` to `launch`.
    ///
    /// # Arguments
    ///
    /// * `launch` - Native launch settings, borrowed until a change is needed.
    /// * `workspace_id` - Placement of the Agent being created or resumed, if any.
    ///
    /// # Returns
    ///
    /// The unchanged launch for an unsandboxed placement, otherwise a confined copy.
    ///
    /// # Errors
    ///
    /// Returns `Registry` when the placement cannot be read, and `ConfinementUnsupported`
    /// when the provider adapter cannot confine its native process.
    pub(super) fn confine<'a>(
        &self,
        mut launch: Cow<'a, AgentSessionSpec>,
        workspace_id: Option<&str>,
    ) -> Result<Cow<'a, AgentSessionSpec>, AgentManagerError> {
        let Some(confinement) = self.confinement(workspace_id)? else {
            return Ok(launch);
        };
        if !self
            .clients
            .get(&launch.provider)
            .is_some_and(|client| client.supports_write_confinement())
        {
            return Err(AgentManagerError::ConfinementUnsupported(
                launch.provider.clone(),
            ));
        }
        launch.to_mut().confinement = Some(confinement);
        Ok(launch)
    }

    /// Confine an owned native control launch, such as a rewind, for `workspace_id`.
    ///
    /// # Errors
    ///
    /// Returns `UnsupportedCapability` for an adapter without confinement support and
    /// `AgentIo` when the placement cannot be read.
    pub(super) fn confine_control(
        &self,
        spec: AgentSessionSpec,
        workspace_id: Option<&str>,
    ) -> Result<AgentSessionSpec, ErrorCode> {
        self.confine(Cow::Owned(spec), workspace_id)
            .map(Cow::into_owned)
            .map_err(|error| match error {
                AgentManagerError::ConfinementUnsupported(_) => ErrorCode::UnsupportedCapability,
                _ => ErrorCode::AgentIo,
            })
    }

    fn confinement(
        &self,
        workspace_id: Option<&str>,
    ) -> Result<Option<WriteConfinement>, AgentManagerError> {
        let (Some(workspaces), Some(id)) = (&self.workspaces, workspace_id) else {
            return Ok(None);
        };
        Ok(workspaces
            .get(id)
            .map_err(|_| AgentManagerError::Registry)?
            .as_ref()
            .and_then(WriteConfinement::for_workspace))
    }
}

#[cfg(test)]
mod tests;
