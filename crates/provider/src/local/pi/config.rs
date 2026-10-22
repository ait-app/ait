use std::collections::BTreeSet;
use std::path::Path;

use domain::agent_runtime::StoredAgentConfig;
use serde_json::{Value, json};

use super::PROVIDER;
use crate::ports::agent_session::{AgentSessionError, AgentSessionSpec};

const KNOWN_THINKING_OPTIONS: [(&str, &str, &str); 7] = [
    ("off", "Off", "No extra reasoning"),
    ("minimal", "Minimal", "Light reasoning"),
    ("low", "Low", "Faster reasoning"),
    ("medium", "Medium", "Balanced reasoning"),
    ("high", "High", "Deeper reasoning"),
    ("xhigh", "XHigh", "Very deep reasoning"),
    ("max", "Max", "Extreme reasoning"),
];

// IDs are opaque to Ait: Pi validates or clamps them, including future and imported levels.
fn valid_thinking_level_id(level: &str) -> bool {
    !level.is_empty() && level.len() <= 512 && !level.chars().any(char::is_control)
}

/// Validate host settings, rejecting unsupported selections before native admission.
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
        || config
            .thinking_option_id
            .as_deref()
            .is_some_and(|level| !valid_thinking_level_id(level))
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

/// Describe the native execution policy without claiming a host permission sandbox.
pub(super) fn modes() -> Vec<Value> {
    vec![
        json!({"id":"default","label":"Native Tools","icon":"ShieldOff",
        "colorTier":"dangerous","isUnattended":true,
        "description":"Uses Pi's native tools and configuration. Pi has no built-in approval sandbox. Extension dialogs are cancelled."}),
    ]
}

/// Project native model facts into the catalog, omitting endpoints and credentials.
/// Returns `Failed` for malformed, duplicate or oversized model identities.
pub(super) fn models(data: &Value, state: &Value) -> Result<Vec<Value>, AgentSessionError> {
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
            let options = thinking_options(model);
            let is_default = state["model"]["provider"] == provider && state["model"]["id"] == id;
            // Native state only establishes the current model's effective default.
            let default = state["thinkingLevel"]
                .as_str()
                .filter(|level| is_default && options.iter().any(|option| option["id"] == *level));
            let mut projected = json!({"provider":PROVIDER,"id":qualified,"label":label,
                "isSelectable":true,
                "isDefault":is_default,
                "thinkingOptions":options});
            if let Some(default) = default {
                for option in projected["thinkingOptions"]
                    .as_array_mut()
                    .into_iter()
                    .flatten()
                {
                    if option["id"] == default {
                        option["isDefault"] = json!(true);
                    }
                }
                projected["defaultThinkingOptionId"] = json!(default);
            }
            Ok(projected)
        })
        .collect()
}

fn thinking_options(model: &Value) -> Vec<Value> {
    if model["reasoning"] != true {
        return Vec::new();
    }
    let mut options: Vec<Value> = KNOWN_THINKING_OPTIONS
        .iter()
        .filter(|(id, _, _)| match model["thinkingLevelMap"].get(*id) {
            Some(Value::Null) => false,
            None => !matches!(*id, "xhigh" | "max"),
            Some(_) => true,
        })
        .map(|(id, label, description)| json!({"id":id,"label":label,"description":description}))
        .collect();
    if let Some(mapping) = model["thinkingLevelMap"].as_object() {
        options.extend(
            mapping
                .iter()
                .filter(|(id, value)| {
                    !value.is_null()
                        && valid_thinking_level_id(id)
                        && !KNOWN_THINKING_OPTIONS
                            .iter()
                            .any(|(known, _, _)| known == id)
                })
                .map(|(id, _)| json!({"id":id,"label":id})),
        );
    }
    options
}
