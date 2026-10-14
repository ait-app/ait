//! Processing speeds are native model catalog facts, never a model-name allowlist.

use domain::agent_runtime::StoredAgentConfig;
use serde_json::{Value, json};

use crate::ports::agent_session::AgentSessionError;

/// Project bounded native speed tiers into a select catalog with an explicit Normal default.
pub(super) fn options(native: &Value) -> Vec<Value> {
    let mut options = vec![json!({"id":"default","label":"Normal","isDefault":true})];
    for tier in native["serviceTiers"].as_array().into_iter().flatten() {
        let Some(id) = tier["id"].as_str().filter(|id| valid_id(id)) else {
            continue;
        };
        let label = tier["name"]
            .as_str()
            .filter(|name| !name.is_empty() && name.len() <= 256)
            .unwrap_or_else(|| label(id));
        append(&mut options, id, label);
    }
    for tier in native["additionalSpeedTiers"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if let Some(id) = tier.as_str().filter(|id| valid_id(id)) {
            append(&mut options, id, label(id));
        }
    }
    options
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn label(id: &str) -> &str {
    match id {
        "fast" | "priority" => "Fast",
        "ultrafast" => "Ultrafast",
        "default" => "Normal",
        _ => id,
    }
}

fn append(options: &mut Vec<Value>, id: &str, label: &str) {
    if options.len() < 64 && !options.iter().any(|option| option["id"] == id) {
        options.push(json!({"id":id,"label":label}));
    }
}

/// Read the explicit speed first, retaining compatibility with persisted Fast toggles.
pub(super) fn selected(config: &StoredAgentConfig) -> &str {
    config
        .feature_values
        .as_ref()
        .and_then(|values| values.get("service_tier"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            if super::controls::fast(config) {
                "fast"
            } else {
                "default"
            }
        })
}

/// Reject tiers absent from the selected model's native catalog.
/// # Errors
/// Returns rejected for unsupported speeds, including legacy Fast without a native fast tier.
pub(super) fn validate(
    config: &StoredAgentConfig,
    options: &[Value],
) -> Result<(), AgentSessionError> {
    let selected = selected(config);
    if options.iter().any(|option| option["id"] == selected)
        || (selected == "fast" && options.iter().any(|option| option["id"] == "priority"))
    {
        Ok(())
    } else {
        Err(AgentSessionError::Rejected)
    }
}

/// Present a speed selector only when the native model advertises an additional tier.
pub(super) fn feature(config: &StoredAgentConfig, options: &[Value]) -> Option<Value> {
    if options.len() <= 1 {
        return None;
    }
    let selected = selected(config);
    let value = if selected == "fast"
        && !options.iter().any(|option| option["id"] == "fast")
        && options.iter().any(|option| option["id"] == "priority")
    {
        "priority"
    } else if validate(config, options).is_ok() {
        selected
    } else {
        "default"
    };
    Some(json!({"id":"service_tier","type":"select","label":"Speed",
        "description":"Choose processing speed. Faster tiers increase usage.",
        "tooltip":"Select speed","icon":"zap","desktopTrigger":"icon",
        "value":value,"options":options}))
}

impl super::CodexClient {
    /// Resolve legacy Fast to the native priority ID without changing explicit selections.
    pub(super) fn service_tier<'a>(&self, config: &'a StoredAgentConfig) -> &'a str {
        let selected = selected(config);
        if selected == "fast"
            && let Ok(catalog) = self.speed_catalog.read()
        {
            let model = catalog.iter().find(|model| {
                config
                    .model
                    .as_ref()
                    .map_or(model["isDefault"] == true, |id| model["id"] == *id)
            });
            if let Some(options) = model.and_then(|model| model["speedOptions"].as_array())
                && !options.iter().any(|option| option["id"] == "fast")
                && options.iter().any(|option| option["id"] == "priority")
            {
                return "priority";
            }
        }
        selected
    }
}

#[cfg(test)]
mod tests;
