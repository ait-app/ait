//! Selective overrides for explicitly restoring a persisted Agent.

use domain::agent_runtime::{PersistedAgentRuntimeRecord, StoredAgentConfig};
use serde::Deserialize;

use super::agent_config::NullableSetting;

/// Fields omitted by a resume request preserve the existing durable configuration.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Overrides {
    /// Provider identity must agree with the persisted handle.
    pub(crate) provider: Option<String>,
    /// Optional new native working directory, without changing Workspace ownership.
    pub(crate) cwd: Option<String>,
    /// Explicit null clears the display title.
    #[serde(default)]
    pub(crate) title: NullableSetting,
    /// Selectively supplied native settings; each feature map replaces the previous map.
    #[serde(flatten)]
    pub(crate) config: StoredAgentConfig,
}

impl Overrides {
    /// Apply the specified settings and reactivate `record`, preserving unrelated metadata.
    pub(crate) fn apply(&self, record: &mut PersistedAgentRuntimeRecord) {
        if let Some(cwd) = &self.cwd {
            record.cwd.clone_from(cwd);
        }
        match &self.title {
            NullableSetting::Unchanged => {}
            NullableSetting::Clear => {
                record.title = None;
                record.title_origin = None;
            }
            NullableSetting::Set(title) => {
                record.title = Some(title.trim().to_owned());
                record.title_origin = None;
            }
        }
        let config = record.config.get_or_insert_with(StoredAgentConfig::default);
        replace(&mut config.mode_id, self.config.mode_id.as_ref());
        replace(&mut config.model, self.config.model.as_ref());
        replace(
            &mut config.thinking_option_id,
            self.config.thinking_option_id.as_ref(),
        );
        replace(
            &mut config.feature_values,
            self.config.feature_values.as_ref(),
        );
        replace(
            &mut config.provider_options,
            self.config.provider_options.as_ref(),
        );
        replace(&mut config.tool_policy, self.config.tool_policy.as_ref());
        replace(
            &mut config.system_prompt,
            self.config.system_prompt.as_ref(),
        );
        replace(&mut config.mcp_servers, self.config.mcp_servers.as_ref());
        record.archived_at = None;
    }
}

fn replace<T: Clone>(current: &mut Option<T>, proposed: Option<&T>) {
    if let Some(value) = proposed {
        *current = Some(value.clone());
    }
}
