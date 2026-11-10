//! Configurable next-turn settings, matching Paseo's nullable patch semantics.

use domain::agent_runtime::StoredAgentConfig;
use serde::Deserialize;

/// Three-state patch value, distinguishing omission from explicit null.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum NullableSetting<T = String> {
    /// The request omitted the field.
    #[default]
    Unchanged,
    /// Explicit null removes the host override.
    Clear,
    /// Replace the override with a selected value.
    Set(T),
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for NullableSetting<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Option::<T>::deserialize(deserializer).map(|value| value.map_or(Self::Clear, Self::Set))
    }
}

impl<T: serde::Serialize> serde::Serialize for NullableSetting<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Set(value) => serializer.serialize_some(value),
            Self::Clear | Self::Unchanged => serializer.serialize_none(),
        }
    }
}

impl<T> NullableSetting<T> {
    /// Omitted patch fields retain the existing value when crossing the internal event bridge.
    pub(crate) fn is_unchanged(&self) -> bool {
        matches!(self, Self::Unchanged)
    }
}

impl<T: Clone> NullableSetting<T> {
    fn apply(&self, current: &mut Option<T>) {
        match self {
            Self::Unchanged => {}
            Self::Clear => *current = None,
            Self::Set(value) => *current = Some(value.clone()),
        }
    }
}

/// An omitted value preserves the setting; an explicit null restores provider inheritance.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ConfigPatch {
    /// Explicit workflow mode; null is rejected by the service.
    #[serde(default)]
    pub(crate) mode_id: NullableSetting,
    /// Feature values merged into existing selections, validated by the native adapter.
    pub(crate) feature_values: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Selected model, or explicit null for the provider's inherited model.
    #[serde(default)]
    pub(crate) model_id: NullableSetting,
    /// Selected thinking option, or explicit null for provider inheritance.
    #[serde(default)]
    pub(crate) thinking_option_id: NullableSetting,
    /// Replace native provider options; null restores native defaults.
    #[serde(default)]
    pub(crate) provider_options:
        NullableSetting<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Replace configured MCP servers; null removes host-provided servers.
    #[serde(default)]
    pub(crate) mcp_servers: NullableSetting<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Replace exact MCP preapprovals; null removes host-provided grants.
    #[serde(default)]
    pub(crate) tool_policy: NullableSetting<serde_json::Value>,
    /// Replace the appended system prompt; null restores the provider prompt.
    #[serde(default)]
    pub(crate) system_prompt: NullableSetting,
}

impl ConfigPatch {
    /// Incorporate the provider's grouped model selection without overwriting unrelated updates.
    /// `proposed` is this patch's input; `resolved` is the adapter's expanded configuration.
    pub(crate) fn resolved(
        &self,
        proposed: &StoredAgentConfig,
        resolved: &StoredAgentConfig,
    ) -> Self {
        let mut patch = self.clone();
        if proposed.model != resolved.model {
            patch.model_id = resolved
                .model
                .clone()
                .map_or(NullableSetting::Clear, NullableSetting::Set);
        }
        if proposed.thinking_option_id != resolved.thinking_option_id {
            patch.thinking_option_id = resolved
                .thinking_option_id
                .clone()
                .map_or(NullableSetting::Clear, NullableSetting::Set);
        }
        for (key, value) in resolved.feature_values.iter().flatten() {
            if proposed
                .feature_values
                .as_ref()
                .and_then(|values| values.get(key))
                != Some(value)
            {
                patch
                    .feature_values
                    .get_or_insert_default()
                    .insert(key.clone(), value.clone());
            }
        }
        patch
    }

    /// Apply only present fields to a copy of `current`, preserving unrelated configuration.
    #[must_use]
    pub(crate) fn apply(&self, current: &StoredAgentConfig) -> StoredAgentConfig {
        let mut next = current.clone();
        self.mode_id.apply(&mut next.mode_id);
        if let Some(features) = &self.feature_values {
            next.feature_values
                .get_or_insert_with(std::collections::BTreeMap::new)
                .extend(features.clone());
        }
        self.model_id.apply(&mut next.model);
        self.thinking_option_id.apply(&mut next.thinking_option_id);
        self.provider_options.apply(&mut next.provider_options);
        self.mcp_servers.apply(&mut next.mcp_servers);
        self.tool_policy.apply(&mut next.tool_policy);
        self.system_prompt.apply(&mut next.system_prompt);
        next
    }
}

#[cfg(test)]
mod tests;
