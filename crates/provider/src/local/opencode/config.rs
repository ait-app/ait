use std::{collections::BTreeMap, path::Path};

use domain::agent_runtime::{StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};

use super::PROVIDER;
use crate::local::acp_transport::Transport;
use crate::ports::agent_session::{AgentSessionError, AgentSessionSpec};

/// Reject unsupported or oversized host choices before starting native execution.
pub(super) fn validate(config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
    if config
        .mode_id
        .as_deref()
        .is_some_and(|id| id.is_empty() || id.len() > 128 || id.chars().any(char::is_control))
        || config.model.as_ref().is_some_and(|model| {
            model.len() > 512
                || model.chars().any(char::is_control)
                || model
                    .split_once('/')
                    .is_none_or(|(provider, model)| provider.is_empty() || model.is_empty())
        })
        || config
            .thinking_option_id
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 128 || id.chars().any(char::is_control))
        || config.feature_values.as_ref().is_some_and(|map| {
            map.iter().any(|(key, value)| {
                key != "permission"
                    || !(value.is_null()
                        || matches!(value.as_str(), Some("allow" | "ask" | "deny")))
            })
        })
        || config
            .provider_options
            .as_ref()
            .is_some_and(|map| !map.is_empty())
        || config.tool_policy.is_some()
        || config.mcp_servers.is_some()
        || config
            .system_prompt
            .as_ref()
            .is_some_and(|text| text.len() > 65_536 || text.contains('\0'))
        || serde_json::to_vec(config)
            .map_err(|_| AgentSessionError::Rejected)?
            .len()
            > 65_536
    {
        return Err(AgentSessionError::Rejected);
    }
    Ok(())
}

/// Validate provider, configuration and an existing canonical absolute working directory.
pub(super) fn validate_spec(spec: &AgentSessionSpec) -> Result<(), AgentSessionError> {
    let directory = Path::new(&spec.cwd);
    if spec.provider != PROVIDER
        || !directory.is_absolute()
        || !directory.is_dir()
        || directory
            .canonicalize()
            .map_err(|_| AgentSessionError::Rejected)?
            != directory
    {
        return Err(AgentSessionError::Rejected);
    }
    validate(&spec.config)
}

/// Return the explicit process permission override, or native inheritance when absent.
pub(super) fn permission(config: &StoredAgentConfig) -> Option<&str> {
    config.feature_values.as_ref()?.get("permission")?.as_str()
}

/// Borrow the native option for a protocol category, when supported.
pub(super) fn option<'a>(options: &'a Value, category: &str) -> Option<&'a Value> {
    options
        .as_array()?
        .iter()
        .find(|option| option["category"] == category && option["type"] == "select")
}

/// Flatten native select groups within catalog bounds; malformed choices fail closed.
pub(super) fn choices(option: &Value) -> Result<Vec<&Value>, AgentSessionError> {
    let options = option["options"]
        .as_array()
        .ok_or(AgentSessionError::Failed)?;
    let mut choices = Vec::new();
    for entry in options {
        if let Some(group) = entry["options"].as_array() {
            choices.extend(group);
        } else {
            choices.push(entry);
        }
        if choices.len() > 4096 {
            return Err(AgentSessionError::Failed);
        }
    }
    for choice in &choices {
        choice["value"]
            .as_str()
            .filter(|value| value.len() <= 1024)
            .ok_or(AgentSessionError::Failed)?;
        text(choice, "name")?;
    }
    Ok(choices)
}

/// Project confirmed native config options for the supplied session identity.
pub(super) fn runtime(id: &str, options: &Value) -> StoredAgentRuntimeInfo {
    let selection = |category| {
        option(options, category)
            .and_then(|option| option["currentValue"].as_str())
            .map(str::to_owned)
    };
    StoredAgentRuntimeInfo {
        provider: PROVIDER.to_owned(),
        session_id: Some(id.to_owned()),
        model: selection("model"),
        thinking_option_id: selection("thought_level"),
        mode_id: selection("mode"),
        extra: Some(BTreeMap::from([(
            "availableModes".into(),
            Value::Array(modes(options)),
        )])),
    }
}

/// Validate and copy a bounded native configuration response, rejecting malformed options.
pub(super) fn state(response: &Value) -> Result<Value, AgentSessionError> {
    let options = response["configOptions"]
        .as_array()
        .filter(|options| options.len() <= 128)
        .ok_or(AgentSessionError::Failed)?;
    for option in options {
        text(option, "id")?;
        if option["type"] == "select" {
            choices(option)?;
            option["currentValue"]
                .as_str()
                .ok_or(AgentSessionError::Failed)?;
        }
    }
    Ok(Value::Array(options.clone()))
}

/// Apply explicit selected values to this session; reject unavailable choices or failed RPCs.
pub(super) async fn apply(
    transport: &mut Transport,
    id: &str,
    options: &mut Value,
    config: &StoredAgentConfig,
) -> Result<(), AgentSessionError> {
    validate(config)?;
    for (category, selected) in [
        ("model", &config.model),
        ("thought_level", &config.thinking_option_id),
        ("mode", &config.mode_id),
    ] {
        let Some(value) = selected else { continue };
        let option = option(options, category).ok_or_else(|| {
            tracing::warn!(
                category,
                "OpenCode ACP does not advertise this configuration option"
            );
            AgentSessionError::Rejected
        })?;
        if !choices(option)?
            .iter()
            .any(|choice| choice["value"] == *value)
        {
            tracing::warn!(
                category,
                "OpenCode ACP does not advertise the requested configuration value"
            );
            return Err(AgentSessionError::Rejected);
        }
        if option["currentValue"] == *value {
            continue;
        }
        let result = transport
            .request(
                "session/set_config_option",
                json!({
            "sessionId":id,"configId":text(option, "id")?,"value":value}),
            )
            .await?;
        *options = state(&result)?;
        if self::option(options, category)
            .is_none_or(|confirmed| confirmed["currentValue"] != *value)
        {
            return Err(AgentSessionError::Failed);
        }
    }
    Ok(())
}

/// Borrow a nonempty native string field; missing, oversized or control text is a failure.
pub(super) fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, AgentSessionError> {
    value[key]
        .as_str()
        .filter(|text| {
            !text.is_empty() && text.len() <= 1024 && !text.chars().any(char::is_control)
        })
        .ok_or(AgentSessionError::Failed)
}

/// Preserve native model, effort and mode without introducing host overrides.
pub(super) fn stored(options: &Value) -> StoredAgentConfig {
    let runtime = runtime("", options);
    StoredAgentConfig {
        model: runtime.model,
        mode_id: runtime.mode_id,
        thinking_option_id: runtime.thinking_option_id,
        ..StoredAgentConfig::default()
    }
}

/// Present only the modes offered by this native session's validated configuration.
pub(super) fn modes(options: &Value) -> Vec<Value> {
    option(options, "mode").and_then(|option| choices(option).ok()).unwrap_or_default()
        .into_iter().map(|choice| json!({"id":choice["value"],"label":choice["name"],
            "icon":if choice["value"] == "build" {"Hammer"} else if choice["value"] == "plan" {"ShieldCheck"} else {"Bot"},
            "colorTier":if choice["value"] == "plan" {"planning"} else {"moderate"}})).collect()
}

/// Describe the explicit native permission override and its current selection.
pub(super) fn features(config: &StoredAgentConfig) -> Vec<Value> {
    vec![
        json!({"id":"permission","type":"select","label":"Permissions",
        "description":"Set OpenCode's tool permission rule from the next turn. Unselected sessions inherit native rules.",
        "tooltip":"Native permissions (next turn)",
        "icon":"shield-check","value":permission(config),
        "options":[{"id":"allow","label":"Allow"},{"id":"ask","label":"Ask"},{"id":"deny","label":"Deny"}]}),
    ]
}
