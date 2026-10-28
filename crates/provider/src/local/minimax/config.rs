use std::path::Path;

use domain::agent_runtime::{StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};

use crate::local::acp_config::{choices, option, text};

use super::PROVIDER;
use crate::local::acp_transport::Transport;
use crate::ports::agent_session::{AgentSessionError, AgentSessionSpec};
use crate::protocol::provider::Details;

/// Validate bounded host settings before native admission; unsupported fields are rejected.
pub(super) fn validate(config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
    if config
        .tool_policy
        .as_ref()
        .is_some_and(|value| !value.is_null())
        || config
            .provider_options
            .as_ref()
            .is_some_and(|options| !options.is_empty())
        || config.feature_values.as_ref().is_some_and(|features| {
            features.iter().any(|(key, value)| {
                key != "permissionMode"
                    || !matches!(
                        value.as_str(),
                        Some("default" | "auto" | "bypassPermissions")
                    )
            })
        })
        || [&config.model, &config.thinking_option_id, &config.mode_id]
            .into_iter()
            .flatten()
            .any(|value| value.len() > 1024 || value.chars().any(char::is_control))
        || [&config.model, &config.thinking_option_id, &config.mode_id]
            .into_iter()
            .flatten()
            .any(String::is_empty)
    {
        return Err(AgentSessionError::Rejected);
    }
    crate::local::configuration::validate(config, PROVIDER)?;
    if config.mcp_servers.iter().flatten().any(|(_, server)| {
        server["type"] == "stdio"
            && server["command"]
                .as_str()
                .is_none_or(|command| !Path::new(command).is_absolute())
    }) {
        return Err(AgentSessionError::Rejected);
    }
    Ok(())
}

/// Validate provider identity and an existing absolute working directory; invalid specs reject.
pub(super) fn validate_spec(spec: &AgentSessionSpec) -> Result<(), AgentSessionError> {
    if spec.provider != PROVIDER
        || !Path::new(&spec.cwd).is_absolute()
        || !Path::new(&spec.cwd).is_dir()
    {
        return Err(AgentSessionError::Rejected);
    }
    validate(&spec.config)
}

/// Project native model, reasoning and mode choices; malformed catalogs fail closed.
pub(super) fn details(options: &Value) -> Result<Details, AgentSessionError> {
    let Some(models) = option(options, "model") else {
        return Ok(Details {
            modes: modes(options)?,
            features: features(options)?,
            ..Details::default()
        });
    };
    let thinking = option(options, "thought_level");
    let thinking_options = thinking
        .map(|option| {
            choices(option).map(|choices| {
                choices
                    .into_iter()
                    .map(|choice| {
                        json!({"id":choice["value"],"label":choice["name"],
            "isDefault":choice["value"] == option["currentValue"]})
                    })
                    .collect::<Vec<_>>()
            })
        })
        .transpose()?
        .unwrap_or_default();
    Ok(Details {
        models: choices(models)?
            .into_iter()
            .map(|choice| {
                let model_thinking: &[Value] = if choice["value"] == models["currentValue"] {
                    &thinking_options
                } else {
                    &[]
                };
                let mut model = json!({"provider":PROVIDER,
                    "id":choice["value"],"label":choice["name"],
                    "isDefault":choice["value"] == models["currentValue"],"isSelectable":true,
                    "thinkingOptions":model_thinking});
                if let Some(description) = choice["description"].as_str() {
                    model["description"] = json!(description);
                }
                if choice["value"] == models["currentValue"]
                    && let Some(default) =
                        thinking.and_then(|option| option["currentValue"].as_str())
                {
                    model["defaultThinkingOptionId"] = json!(default);
                }
                model
            })
            .collect(),
        modes: modes(options)?,
        features: features(options)?,
    })
}

/// Return confirmed native selections and modes for the supplied session identity.
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
        extra: Some(std::collections::BTreeMap::from([(
            "availableModes".into(),
            json!(modes(options).unwrap_or_default()),
        )])),
    }
}

/// Apply selected ACP options and verify their confirmed values; unsupported choices reject.
/// Uses `transport` and native session `id`; updates `options` with acknowledged `config`.
/// Returns a transport/protocol error if native acknowledgement fails.
pub(super) async fn apply(
    transport: &mut Transport,
    id: &str,
    options: &mut Value,
    config: &StoredAgentConfig,
) -> Result<(), AgentSessionError> {
    validate(config)?;
    let permission = config
        .feature_values
        .as_ref()
        .and_then(|features| features.get("permissionMode"))
        .and_then(Value::as_str);
    for (category, selected) in [
        ("model", config.model.as_deref()),
        ("thought_level", config.thinking_option_id.as_deref()),
        ("mode", config.mode_id.as_deref()),
        ("_permission", permission),
    ] {
        let Some(value) = selected else { continue };
        let selected_option = option(options, category).ok_or(AgentSessionError::Rejected)?;
        if !choices(selected_option)?
            .iter()
            .any(|choice| choice["value"] == *value)
        {
            return Err(AgentSessionError::Rejected);
        }
        if selected_option["currentValue"] == *value {
            continue;
        }
        if category == "mode" {
            transport
                .request("session/set_mode", json!({"sessionId":id,"modeId":value}))
                .await?;
            update_mode(options, value)?;
            continue;
        }
        let result = transport
            .request(
                "session/set_config_option",
                json!({
            "sessionId":id,"configId":text(selected_option, "id")?,"value":value}),
            )
            .await?;
        *options = state(&result, Some(options))?;
        if option(options, category).is_none_or(|option| option["currentValue"] != *value) {
            return Err(AgentSessionError::Failed);
        }
    }
    Ok(())
}

/// Project modes from the native ACP config options without inventing permission scopes.
/// Returns protocol failure for malformed choices.
pub(super) fn modes(options: &Value) -> Result<Vec<Value>, AgentSessionError> {
    let Some(mode) = option(options, "mode") else {
        return Ok(Vec::new());
    };
    choices(mode)?
        .into_iter()
        .map(|choice| {
            Ok(json!({
                "id": choice["value"], "label": choice["name"],
                "description": choice.get("description").and_then(Value::as_str).unwrap_or(""),
                "isDefault": choice["value"] == mode["currentValue"],
            "icon": if choice["value"] == "plan" {"ShieldCheck"} else {"Bot"},
            "colorTier": if choice["value"] == "plan" {"planning"} else {"moderate"}
            }))
        })
        .collect()
}

/// Normalize ACP's separate modes and configOptions, preserving modes on config-only updates.
/// Returns failure for malformed catalogs or native mode identities.
pub(super) fn state(
    response: &Value,
    previous: Option<&Value>,
) -> Result<Value, AgentSessionError> {
    let mut options = crate::local::acp_config::state(response)?;
    let modes = if response["modes"].is_object() {
        let modes = &response["modes"];
        let choices = modes["availableModes"]
            .as_array()
            .filter(|choices| choices.len() <= 128)
            .ok_or(AgentSessionError::Failed)?;
        let current = text(modes, "currentModeId")?;
        let choices = choices
            .iter()
            .map(|mode| {
                Ok(json!({
                    "value":text(mode,"id")?,
                    "name":text(mode,"name")?,
                    "description":mode.get("description").and_then(Value::as_str).unwrap_or("")
                }))
            })
            .collect::<Result<Vec<_>, AgentSessionError>>()?;
        if !choices.iter().any(|choice| choice["value"] == current) {
            return Err(AgentSessionError::Failed);
        }
        Some(json!({
            "id":"ait-native-mode","category":"mode","type":"select",
            "currentValue":current,"options":choices
        }))
    } else {
        previous
            .and_then(|previous| option(previous, "mode"))
            .cloned()
    };
    if let Some(modes) = modes {
        options
            .as_array_mut()
            .ok_or(AgentSessionError::Failed)?
            .push(modes);
    }
    Ok(options)
}

/// Apply a native mode acknowledgement to the existing catalog; unknown identities fail closed.
pub(super) fn update_mode(options: &mut Value, mode: &str) -> Result<(), AgentSessionError> {
    let option = options
        .as_array_mut()
        .and_then(|options| {
            options
                .iter_mut()
                .find(|option| option["category"] == "mode")
        })
        .ok_or(AgentSessionError::Failed)?;
    if !choices(option)?
        .iter()
        .any(|choice| choice["value"] == mode)
    {
        return Err(AgentSessionError::Failed);
    }
    option["currentValue"] = json!(mode);
    Ok(())
}

/// Expose native process permission choices separately from ACP workflow modes.
/// Returns protocol failure for malformed permission options.
pub(super) fn features(options: &Value) -> Result<Vec<Value>, AgentSessionError> {
    let Some(permission) = option(options, "_permission") else {
        return Ok(Vec::new());
    };
    let choices = choices(permission)?
        .into_iter()
        .map(|choice| json!({"id":choice["value"],"label":choice["name"]}))
        .collect::<Vec<_>>();
    Ok(vec![json!({
        "id":"permissionMode","type":"select","label":"Permissions",
        "tooltip":"MiniMax Code native tool permissions","icon":"shield-check",
        "value":permission["currentValue"],"options":choices
    })])
}

/// Report implemented controls with native choices; an empty catalog is valid before discovery.
pub(super) fn settings(options: &Value) -> Value {
    json!({
        "availableModes":modes(options).unwrap_or_default(),
        "features":features(options).unwrap_or_default(),
        "capabilities":{
            "supportsStreaming":true,"supportsReasoningStream":true,
            "supportsMcpServers":true,"supportsDynamicModes":true,
            "supportsSessionListing":false,"supportsRewindConversation":false,
            "supportsRewindFiles":false,"supportsRewindBoth":false}})
}
