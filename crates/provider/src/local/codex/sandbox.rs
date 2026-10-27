//! Preserve native resolved permissions instead of rebuilding them from UI presets.

use domain::agent_runtime::StoredAgentConfig;
use serde_json::{Value, json};

use super::{Transport, controls};
use crate::ports::agent_session::AgentSessionError;

/// Inherited and effective permissions owned by one native thread connection.
#[derive(Debug, Default)]
pub(super) struct Sandbox {
    workspace_write: Value,
    effective: Option<Value>,
}

impl Sandbox {
    /// Read inherited workspace permissions from the native configuration for `cwd`.
    /// Older servers may reject this optional inspection; transport failures propagate.
    pub(super) async fn load(
        transport: &mut Transport,
        cwd: &str,
    ) -> Result<Self, AgentSessionError> {
        match transport
            .request("config/read", json!({"cwd":cwd,"includeLayers":false}))
            .await
        {
            Ok(response) => Ok(Self {
                workspace_write: response["config"]["sandbox_workspace_write"].clone(),
                effective: None,
            }),
            Err(AgentSessionError::Rejected) => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    /// Retain the authoritative sandbox returned by native thread start or resume.
    pub(super) fn observe(&mut self, response: &Value) {
        self.effective = response
            .get("sandbox")
            .filter(|policy| policy.is_object())
            .cloned();
        if let Some(policy) = &self.effective
            && policy["type"] == "workspaceWrite"
        {
            self.workspace_write = policy.clone();
        }
    }

    /// Resolve a turn policy, preserving native settings for a matching permission mode.
    /// Explicit workspace options override inherited defaults when changing sandbox type.
    pub(super) fn policy(&self, config: &StoredAgentConfig) -> Value {
        let mut policy = controls::policy(config).2;
        if let Some(effective) = &self.effective
            && effective["type"] == policy["type"]
        {
            return effective.clone();
        }
        if policy["type"] == "workspaceWrite" {
            let explicit = config
                .provider_options
                .as_ref()
                .and_then(|options| options.get("sandbox_workspace_write"));
            for (source, target) in controls::WORKSPACE_FIELDS {
                if let Some(value) = explicit
                    .and_then(|options| options.get(source))
                    .or_else(|| self.workspace_write.get(source))
                    .or_else(|| self.workspace_write.get(target))
                {
                    policy[target] = value.clone();
                }
            }
        }
        policy
    }
}

#[cfg(test)]
mod tests;
