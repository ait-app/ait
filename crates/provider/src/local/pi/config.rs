use std::collections::BTreeSet;
use std::path::Path;

use domain::agent_runtime::StoredAgentConfig;
use serde_json::{Value, json};

use super::PROVIDER;
use crate::ports::agent_session::{AgentSessionError, AgentSessionSpec};

/// Validate host settings, rejecting unsupported selections before native admission.
/// The retired `default` mode remains accepted only for persisted Agent compatibility.
/// Returns `Rejected` for settings this adapter cannot represent.
pub(super) fn validate(config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
    if config
        .mode_id
        .as_deref()
        .is_some_and(|mode| mode != "default")
        || config
            .model
            .as_deref()
            .is_some_and(|model| split_model(model).is_none())
        || config.thinking_option_id.is_some()
        || config.system_prompt.is_some()
        || config
            .tool_policy
            .as_ref()
            .is_some_and(|value| !value.is_null())
        || config
            .mcp_servers
            .as_ref()
            .is_some_and(|value| !value.is_empty())
        || config
            .provider_options
            .as_ref()
            .is_some_and(|value| !value.is_empty())
        || config
            .feature_values
            .as_ref()
            .is_some_and(|value| !value.is_empty())
    {
        return Err(AgentSessionError::Rejected);
    }
    Ok(())
}

/// Split a bounded, qualified model identity; malformed identities return `None`.
pub(super) fn split_model(model: &str) -> Option<(&str, &str)> {
    let (provider, id) = model.split_once('/')?;
    (!provider.is_empty()
        && !id.is_empty()
        && model.len() <= 512
        && !model.chars().any(char::is_control))
    .then_some((provider, id))
}

/// Require an existing absolute working directory, otherwise return `Rejected`.
pub(super) fn directory(cwd: &str) -> Result<(), AgentSessionError> {
    if !Path::new(cwd).is_absolute() || !Path::new(cwd).is_dir() {
        return Err(AgentSessionError::Rejected);
    }
    Ok(())
}

/// Validate the provider identity, working directory and requested configuration.
/// Returns `Rejected` before starting a process for invalid specifications.
pub(super) fn spec(spec: &AgentSessionSpec) -> Result<(), AgentSessionError> {
    if spec.provider != PROVIDER {
        return Err(AgentSessionError::Rejected);
    }
    directory(&spec.cwd)?;
    validate(&spec.config)
}

/// Project native model facts into the catalog, omitting endpoints and credentials.
/// Returns `Failed` for malformed, duplicate or oversized model identities.
pub(super) fn models(data: &Value) -> Result<Vec<Value>, AgentSessionError> {
    let models = data["models"]
        .as_array()
        .filter(|models| models.len() <= 4096)
        .ok_or(AgentSessionError::Failed)?;
    let mut seen = BTreeSet::new();
    models
        .iter()
        .map(|model| {
            let provider = model["provider"]
                .as_str()
                .ok_or(AgentSessionError::Failed)?;
            let id = model["id"].as_str().ok_or(AgentSessionError::Failed)?;
            let qualified = format!("{provider}/{id}");
            if split_model(&qualified).is_none() || !seen.insert(qualified.clone()) {
                return Err(AgentSessionError::Failed);
            }
            let label = model
                .get("name")
                .map_or(Some(id), Value::as_str)
                .filter(|name| {
                    !name.is_empty() && name.len() <= 1024 && !name.chars().any(char::is_control)
                })
                .ok_or(AgentSessionError::Failed)?;
            Ok(json!({"provider":PROVIDER,"id":qualified,"label":label,
            "isSelectable":true,"thinkingOptions":[]}))
        })
        .collect()
}
