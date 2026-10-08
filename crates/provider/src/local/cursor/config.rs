use std::collections::BTreeSet;
use std::path::Path;

use domain::agent_runtime::{StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};

use super::PROVIDER;
use crate::local::acp::{text, transport::Transport};
use crate::ports::agent_session::{AgentSessionError, AgentSessionSpec};
use crate::protocol::provider::Details;

/// Validate `config` before admission; reject unsupported or oversized native settings.
pub(super) fn validate(config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
    crate::local::configuration::validate(config, PROVIDER)?;
    if config.system_prompt.is_some()
        || config
            .tool_policy
            .as_ref()
            .is_some_and(|value| !value.is_null())
        || config
            .provider_options
            .as_ref()
            .is_some_and(|values| !values.is_empty())
        || config.feature_values.as_ref().is_some_and(|values| {
            values
                .iter()
                .any(|(key, value)| key != "fast_mode" || !value.is_boolean())
        })
        || config
            .mcp_servers
            .as_ref()
            .is_some_and(|values| !values.is_empty())
        || [&config.model, &config.mode_id, &config.thinking_option_id]
            .into_iter()
            .flatten()
            .any(|value| {
                value.is_empty() || value.len() > 1024 || value.chars().any(char::is_control)
            })
    {
        return Err(AgentSessionError::Rejected);
    }
    Ok(())
}

/// Validate Cursor identity, an existing absolute cwd and config; reject invalid specs.
pub(super) fn validate_spec(spec: &AgentSessionSpec) -> Result<(), AgentSessionError> {
    if spec.provider != PROVIDER
        || !Path::new(&spec.cwd).is_absolute()
        || !Path::new(&spec.cwd).is_dir()
    {
        return Err(AgentSessionError::Rejected);
    }
    validate(&spec.config)
}

/// Find the select option in `options` for `category`, or return None when absent.
pub(super) fn option<'a>(options: &'a Value, category: &str) -> Option<&'a Value> {
    options.as_array()?.iter().find(|option| {
        (option["category"] == category || option["id"] == category) && option["type"] == "select"
    })
}

fn choices(option: &Value) -> Result<Vec<&Value>, AgentSessionError> {
    let entries = option["options"]
        .as_array()
        .ok_or(AgentSessionError::Failed)?;
    let mut choices = Vec::new();
    for entry in entries {
        if let Some(group) = entry["options"].as_array() {
            choices.extend(group);
        } else {
            choices.push(entry);
        }
        if choices.len() > 4096 {
            return Err(AgentSessionError::Failed);
        }
    }
    let mut ids = BTreeSet::new();
    for choice in &choices {
        if !ids.insert(text(choice, "value")?) {
            return Err(AgentSessionError::Failed);
        }
        text(choice, "name")?;
    }
    Ok(choices)
}

/// Normalize ACP `response` configuration; fail on malformed or oversized catalogs.
pub(super) fn state(response: &Value) -> Result<Value, AgentSessionError> {
    let mut options = if let Some(options) = response.get("configOptions") {
        options
            .as_array()
            .filter(|options| options.len() <= 128)
            .ok_or(AgentSessionError::Failed)?
            .clone()
    } else {
        Vec::new()
    };
    for (field, category, list, current, id) in [
        (
            "models",
            "model",
            "availableModels",
            "currentModelId",
            "modelId",
        ),
        ("modes", "mode", "availableModes", "currentModeId", "id"),
    ] {
        if options.iter().any(|option| option["category"] == category) {
            continue;
        }
        if let Some(native) = response.get(field) {
            let entries = native[list]
                .as_array()
                .filter(|entries| entries.len() <= 4096)
                .ok_or(AgentSessionError::Failed)?;
            options.push(json!({"id":category,"category":category,"type":"select",
                "currentValue":text(native,current)?,"legacy":category,
                "options":entries.iter().map(|entry| Ok(json!({"value":text(entry,id)?,
                    "name":text(entry,"name")?,"description":entry["description"]})))
                    .collect::<Result<Vec<_>,AgentSessionError>>()?}));
        }
    }
    let mut ids = BTreeSet::new();
    for option in &options {
        if !ids.insert(text(option, "id")?) {
            return Err(AgentSessionError::Failed);
        }
        if option["type"] == "select" {
            choices(option)?;
            text(option, "currentValue")?;
        }
    }
    Ok(Value::Array(options))
}

/// One model's exact ACP identifier and its independently reported parameters.
#[derive(Debug)]
pub(super) struct Model {
    id: String,
    label: String,
    options: Value,
}

/// Parse Cursor's read-only model `response`; fail on malformed or duplicate entries.
pub(super) fn catalog(response: &Value) -> Result<Vec<Model>, AgentSessionError> {
    let models = response["models"]
        .as_array()
        .filter(|models| models.len() <= 4096)
        .ok_or(AgentSessionError::Failed)?;
    let mut ids = BTreeSet::new();
    models
        .iter()
        .map(|model| {
            let id = text(model, "value")?;
            if !ids.insert(id) || model.get("configOptions").is_none() {
                return Err(AgentSessionError::Failed);
            }
            Ok(Model {
                id: id.to_owned(),
                label: text(model, "name")?.to_owned(),
                options: state(model)?,
            })
        })
        .collect()
}

/// Replace the session's partial model picker with `catalog` without changing its selection.
pub(super) fn expand_models(options: &mut Value, catalog: &[Model]) {
    if let Some(models) = options.as_array_mut().and_then(|options| {
        options
            .iter_mut()
            .find(|option| option["category"] == "model")
    }) {
        models["options"] = json!(
            catalog
                .iter()
                .map(|model| { json!({"value":model.id,"name":model.label}) })
                .collect::<Vec<_>>()
        );
    }
}

/// Merge native `response` while retaining model/mode selectors from the legacy protocol.
/// Returns failure for malformed state; an empty configuration array removes stale parameters.
pub(super) fn merge(options: &mut Value, response: &Value) -> Result<(), AgentSessionError> {
    let incoming = state(response)?;
    let incoming = incoming.as_array().ok_or(AgentSessionError::Failed)?;
    let entries = options.as_array_mut().ok_or(AgentSessionError::Failed)?;
    entries.retain(|entry| {
        (response.get("configOptions").is_none()
            || entry["legacy"].is_string()
            || matches!(entry["category"].as_str(), Some("model" | "mode")))
            && !incoming.iter().any(|option| {
                option["id"] == entry["id"]
                    || (option["category"].is_string() && option["category"] == entry["category"])
            })
    });
    entries.reserve(incoming.len());
    for entry in incoming {
        entries.push(entry.clone());
    }
    Ok(())
}

fn model_options<'a>(
    options: &'a Value,
    catalog: &'a [Model],
    config: &StoredAgentConfig,
) -> &'a Value {
    let selected = config
        .model
        .as_deref()
        .or_else(|| option(options, "model").and_then(|model| model["currentValue"].as_str()));
    catalog
        .iter()
        .find(|model| Some(model.id.as_str()) == selected)
        .map_or(options, |model| &model.options)
}

fn validate_choice(
    options: &Value,
    category: &str,
    selected: &str,
) -> Result<(), AgentSessionError> {
    let selector = option(options, category).ok_or(AgentSessionError::Rejected)?;
    if choices(selector)?
        .iter()
        .any(|choice| choice["value"] == selected)
    {
        Ok(())
    } else {
        Err(AgentSessionError::Rejected)
    }
}

/// Validate selections using `options` and per-model `catalog`; no native preferences are written.
pub(super) fn validate_selection(
    options: &Value,
    catalog: &[Model],
    config: &StoredAgentConfig,
) -> Result<(), AgentSessionError> {
    validate(config)?;
    for (category, selection) in [("model", &config.model), ("mode", &config.mode_id)] {
        if let Some(selection) = selection {
            validate_choice(options, category, selection)?;
        }
    }
    let parameters = model_options(options, catalog, config);
    if let Some(thinking) = &config.thinking_option_id {
        validate_choice(parameters, "thought_level", thinking)?;
    }
    if let Some(fast) = fast(config) {
        if option(parameters, "fast").is_some() {
            validate_choice(parameters, "fast", if fast { "true" } else { "false" })?;
        } else if fast {
            return Err(AgentSessionError::Rejected);
        }
    }
    Ok(())
}

fn fast(config: &StoredAgentConfig) -> Option<bool> {
    config
        .feature_values
        .as_ref()
        .and_then(|values| values.get("fast_mode"))
        .and_then(Value::as_bool)
}

/// Describe model-dependent controls using `options`, `catalog` and proposed `config` without I/O.
pub(super) fn features(
    options: &Value,
    catalog: &[Model],
    config: &StoredAgentConfig,
) -> Vec<Value> {
    let parameters = model_options(options, catalog, config);
    let Some(selector) = option(parameters, "fast") else {
        return Vec::new();
    };
    let current_model = option(options, "model").map(|model| &model["currentValue"]);
    let current = if config
        .model
        .as_ref()
        .is_none_or(|model| current_model.is_some_and(|current| *current == *model))
    {
        option(options, "fast").unwrap_or(selector)
    } else {
        selector
    };
    vec![json!({"id":"fast_mode","type":"toggle","label":"Fast",
        "description":"Cursor fast mode","tooltip":"Toggle Cursor fast mode","icon":"zap",
        "value":fast(config).unwrap_or(current["currentValue"] == "true")})]
}

/// Project native `options` and per-model `catalog`; fail on malformed choices.
pub(super) fn details(options: &Value, catalog: &[Model]) -> Result<Details, AgentSessionError> {
    let mut details = Details::default();
    let current = option(options, "model").map(|option| &option["currentValue"]);
    details.models = catalog.iter().map(|model| {
        let thinking = option(&model.options, "thought_level");
        let thinking_options = thinking.map(choices).transpose()?.unwrap_or_default()
            .into_iter().map(|choice| json!({"id":choice["value"],"label":choice["name"],
                "isDefault":thinking.is_some_and(|option| choice["value"] == option["currentValue"])}))
            .collect::<Vec<_>>();
        let mut result = json!({"provider":PROVIDER,"id":model.id,"label":model.label,
            "isDefault":current.is_some_and(|current| *current == model.id),"isSelectable":true,
            "thinkingOptions":thinking_options,"supportsFastMode":option(&model.options,"fast").is_some()});
        if let Some(default) = thinking.and_then(|option| option["currentValue"].as_str()) {
            result["defaultThinkingOptionId"] = json!(default);
        }
        Ok(result)
    }).collect::<Result<Vec<_>,AgentSessionError>>()?;
    if let Some(modes) = option(options, "mode") {
        details.modes = choices(modes)?
            .into_iter()
            .map(|choice| {
                let (icon, color) = match choice["value"].as_str() {
                    Some("plan") => ("ShieldEllipsis", "planning"),
                    Some("ask") => ("Shield", "safe"),
                    _ => ("Hammer", "moderate"),
                };
                json!({"id":choice["value"],"label":choice["name"],"icon":icon,"colorTier":color,
                "isDefault":choice["value"]==modes["currentValue"]})
            })
            .collect();
    }
    Ok(details)
}

/// Project native `id` and current `options` into the runtime snapshot.
pub(super) fn runtime(id: &str, options: &Value) -> StoredAgentRuntimeInfo {
    let selected = |category| {
        option(options, category)
            .and_then(|option| option["currentValue"].as_str())
            .map(str::to_owned)
    };
    StoredAgentRuntimeInfo {
        provider: PROVIDER.to_owned(),
        session_id: Some(id.to_owned()),
        model: selected("model"),
        mode_id: selected("mode"),
        thinking_option_id: selected("thought_level"),
        extra: None,
    }
}

/// Apply selections for session `id` using per-model `catalog`; reject invalid selections before mutation.
pub(super) async fn apply(
    transport: &mut Transport,
    id: &str,
    options: &mut Value,
    catalog: &[Model],
    config: &StoredAgentConfig,
) -> Result<(), AgentSessionError> {
    validate_selection(options, catalog, config)?;
    let fast = fast(config).map(|fast| if fast { "true" } else { "false" }.to_owned());
    for (category, selected) in [
        ("model", &config.model),
        ("mode", &config.mode_id),
        ("thought_level", &config.thinking_option_id),
        ("fast", &fast),
    ] {
        let Some(value) = selected else { continue };
        let Some(selector) = option(options, category) else {
            if category == "fast" && value == "false" {
                continue;
            }
            return Err(AgentSessionError::Rejected);
        };
        validate_choice(options, category, value)?;
        if selector["currentValue"] == *value {
            continue;
        }
        let legacy = selector["legacy"].is_string();
        let (method, params) = match selector["legacy"].as_str() {
            Some("model") => ("session/set_model", json!({"sessionId":id,"modelId":value})),
            Some("mode") => ("session/set_mode", json!({"sessionId":id,"modeId":value})),
            _ => (
                "session/set_config_option",
                json!({"sessionId":id,"configId":text(selector,"id")?,"value":value}),
            ),
        };
        let response = transport.request(method, params).await?;
        let mut updated_parameters = false;
        transport.consume_notifications(|message| {
            if message["method"] != "session/update" {
                return Ok(false);
            }
            if message["params"]["sessionId"] != id {
                return Err(AgentSessionError::Failed);
            }
            let update = &message["params"]["update"];
            if update["sessionUpdate"] == "config_option_update" {
                merge(options, update)?;
                expand_models(options, catalog);
                updated_parameters = true;
                Ok(true)
            } else if update["sessionUpdate"] == "current_mode_update" {
                let mode = text(update, "currentModeId")?;
                if let Some(selector) = options.as_array_mut().and_then(|entries| {
                    entries.iter_mut().find(|entry| entry["category"] == "mode")
                }) {
                    selector["currentValue"] = json!(mode);
                }
                Ok(true)
            } else {
                Ok(false)
            }
        })?;
        merge(options, &response)?;
        expand_models(options, catalog);
        if legacy {
            let entry = options
                .as_array_mut()
                .ok_or(AgentSessionError::Failed)?
                .iter_mut()
                .find(|entry| entry["category"] == category)
                .ok_or(AgentSessionError::Failed)?;
            entry["currentValue"] = json!(value);
        } else if option(options, category).is_none_or(|entry| entry["currentValue"] != *value) {
            return Err(AgentSessionError::Failed);
        }
        // Legacy set_model can return {} and publish updated parameters asynchronously.
        // Use the read-only model metadata until an authoritative config update arrives.
        if category == "model" && response.get("configOptions").is_none() && !updated_parameters {
            let model = catalog
                .iter()
                .find(|model| model.id == *value)
                .ok_or(AgentSessionError::Failed)?;
            merge(options, &json!({"configOptions":model.options}))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
