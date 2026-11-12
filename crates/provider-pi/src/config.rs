use std::collections::BTreeSet;
use std::path::Path;

use domain::agent_runtime::StoredAgentConfig;
use serde_json::{Value, json};

use super::PROVIDER;
use provider_core::ports::agent_session::{AgentSessionError, AgentSessionSpec};

const KNOWN_THINKING_OPTIONS: [(&str, &str, &str); 7] = [
    (
        "off",
        "Off",
        "Request Pi's Off level; endpoint behavior is not verified",
    ),
    ("minimal", "Minimal", "Light reasoning"),
    ("low", "Low", "Faster reasoning"),
    ("medium", "Medium", "Balanced reasoning"),
    ("high", "High", "Deeper reasoning"),
    ("xhigh", "XHigh", "Very deep reasoning"),
    ("max", "Max", "Extreme reasoning"),
];

// IDs are opaque to Ait: Pi validates or clamps them, including future and imported levels.
/// Accept nonempty opaque levels up to 512 bytes without control characters.
pub(super) fn valid_thinking_level_id(level: &str) -> bool {
    !level.is_empty() && level.len() <= 512 && !level.chars().any(char::is_control)
}

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
            .iter()
            .flatten()
            .any(|(_, server)| !provider_native::acp_config::launchable(server))
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
    // Bounded MCP definitions reach the session's private extension.
    provider_native::configuration::validate(config, PROVIDER)
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

/// Read a native model's provider and ID, ignoring other metadata.
/// Returns `None` when either identity field is missing or is not a string.
pub(super) fn model_identity(model: &Value) -> Option<(&str, &str)> {
    model["provider"].as_str().zip(model["id"].as_str())
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
pub(super) fn models(data: &Value, state: &Value) -> Result<Vec<Value>, AgentSessionError> {
    let entries = model_entries(data)?;
    data["models"]
        .as_array()
        .into_iter()
        .flatten()
        .zip(entries)
        .enumerate()
        .map(|(index, (model, (qualified, label)))| {
            let options = catalog_thinking_options(model);
            // Seed every model from the ambient level, clamped to its own capabilities.
            let default = state["thinkingLevel"]
                .as_str()
                .filter(|level| valid_thinking_level_id(level))
                .and_then(|level| {
                    default_thinking_option(options.clone().map(|(id, _, _)| id), level)
                });
            let options: Vec<_> = options
                .map(|(id, label, description)| {
                    let mut option = json!({"id":id,"label":label,"description":description});
                    if default == Some(id) {
                        option["isDefault"] = json!(true);
                    }
                    option
                })
                .collect();
            let mut projected = json!({"provider":PROVIDER,"id":qualified,"label":label,
                "isSelectable":true,
                "isDefault":index == 0,
                "thinkingOptions":options});
            if let Some(default) = default {
                projected["defaultThinkingOptionId"] = json!(default);
            }
            Ok(projected)
        })
        .collect()
}

/// Validate the native catalog and return only qualified IDs, without projecting options.
/// Returns `Failed` for malformed, duplicate or oversized identities or invalid labels.
pub(super) fn model_ids(data: &Value) -> Result<Vec<String>, AgentSessionError> {
    Ok(model_entries(data)?.into_iter().map(|(id, _)| id).collect())
}

// Validate once and retain labels so projection cannot reinterpret the name/ID fallback.
fn model_entries(data: &Value) -> Result<Vec<(String, &str)>, AgentSessionError> {
    let models = data["models"]
        .as_array()
        .filter(|models| models.len() <= 4096)
        .ok_or(AgentSessionError::Failed)?;
    let mut seen = BTreeSet::new();
    models
        .iter()
        .map(|model| {
            let (provider, id) = model_identity(model).ok_or(AgentSessionError::Failed)?;
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
            Ok((qualified, label))
        })
        .collect()
}

fn default_thinking_option<'a>(
    mut options: impl Iterator<Item = &'a str> + Clone,
    level: &str,
) -> Option<&'a str> {
    let supported = |id: &str| options.clone().find(|option| *option == id);
    let clamped = supported(level).or_else(|| {
        let index = KNOWN_THINKING_OPTIONS
            .iter()
            .position(|(id, _, _)| *id == level)?;
        // Pi tries higher levels first, then lower levels in descending order.
        KNOWN_THINKING_OPTIONS[index..]
            .iter()
            .chain(KNOWN_THINKING_OPTIONS[..index].iter().rev())
            .find_map(|(id, _, _)| supported(id))
    });
    clamped.or_else(|| options.next())
}

/// Clamp `level` to `model`'s supported ladder; non-reasoning models use off.
/// Preserve the seed for Pi to validate when reasoning capabilities are unknown.
/// Returns `None` only when known reasoning capabilities exclude every selectable level.
pub(super) fn clamp_thinking_level(model: &Value, level: &str) -> Option<String> {
    match model["reasoning"].as_bool() {
        Some(false) => Some("off".to_owned()),
        Some(true) => default_thinking_option(thinking_options(model).map(|(id, _, _)| id), level)
            .map(str::to_owned),
        None => Some(level.to_owned()),
    }
}

fn thinking_options(
    model: &Value,
) -> impl Iterator<Item = (&'static str, &'static str, &'static str)> + Clone + '_ {
    KNOWN_THINKING_OPTIONS
        .iter()
        .copied()
        .filter(|_| model["reasoning"] == true)
        .filter(|(id, _, _)| match model["thinkingLevelMap"].get(*id) {
            Some(Value::Null) => false,
            None => !matches!(*id, "xhigh" | "max"),
            Some(_) => true,
        })
}

// This conservative picker hint is not a delivery capability or a native clamping rule.
fn catalog_thinking_options(
    model: &Value,
) -> impl Iterator<Item = (&'static str, &'static str, &'static str)> + Clone + '_ {
    let offer_off = match model["thinkingLevelMap"].get("off") {
        Some(value) => value.as_str().is_some_and(|level| !level.is_empty()),
        None => model["api"] != "openai-completions",
    };
    thinking_options(model).filter(move |(id, _, _)| offer_off || *id != "off")
}
