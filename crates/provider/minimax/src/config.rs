use std::path::Path;

use domain::agent_runtime::{StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};

use provider_native::acp_config::{choices, option, text};

use super::PROVIDER;
use super::catalog::{self, Probe, Selection};
use provider_core::ports::agent_session::{AgentSessionError, AgentSessionSpec};
use provider_core::protocol::provider::Details;
use provider_native::acp_transport::Transport;

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
    provider_native::configuration::validate(config, PROVIDER)?;
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

/// Project probed native models, plus mode and permission choices from `options`.
///
/// `options` is the native catalog before probing and `probes` records each advertised model
/// choice; models split native reasoning variants into thinking options.
///
/// # Errors
///
/// Returns `AgentSessionError::Failed` for malformed catalogs.
pub(super) fn details(options: &Value, probes: &[Probe]) -> Result<Details, AgentSessionError> {
    let models = match option(options, "model") {
        Some(native) => catalog::models(text(native, "currentValue")?, probes)?,
        None => Vec::new(),
    };
    Ok(Details {
        models,
        modes: modes(options)?,
        features: features(options)?,
    })
}

/// Return confirmed native selections and modes for the supplied session identity.
pub(super) fn runtime(id: &str, options: &Value) -> StoredAgentRuntimeInfo {
    let selection =
        |category| option(options, category).and_then(|option| option["currentValue"].as_str());
    let (model, variant) = selection("model").map_or((None, None), |model| {
        let (model, variant) = catalog::split(model);
        (Some(model.to_owned()), variant)
    });
    StoredAgentRuntimeInfo {
        provider: PROVIDER.to_owned(),
        session_id: Some(id.to_owned()),
        model,
        thinking_option_id: catalog::thinking_id(variant, selection("thought_level")),
        mode_id: selection("mode").map(str::to_owned),
        extra: Some(std::collections::BTreeMap::from([(
            "availableModes".into(),
            json!(modes(options).unwrap_or_default()),
        )])),
    }
}

/// Apply selected ACP options and verify their confirmed values; unsupported choices reject.
/// Uses `transport` and native session `id`; updates `options` with acknowledged `config`.
/// The host model and thinking choice resolve to a native model variant and effort first.
/// Returns a transport/protocol error if native acknowledgement fails.
pub(super) async fn apply(
    transport: &mut Transport,
    id: &str,
    options: &mut Value,
    config: &StoredAgentConfig,
) -> Result<(), AgentSessionError> {
    validate(config)?;
    let model = config.model.as_deref();
    let thinking = config.thinking_option_id.as_deref();
    let selection = match option(options, "model") {
        Some(native) => catalog::resolve(native, model, thinking)?,
        None if model.is_some() => return Err(AgentSessionError::Rejected),
        None => Selection {
            models: Vec::new(),
            effort: thinking.map(str::to_owned),
        },
    };
    let last = selection.models.len().saturating_sub(1);
    for (index, value) in selection.models.iter().enumerate() {
        match select(transport, id, options, "model", value).await {
            Err(AgentSessionError::Rejected) if index < last => {}
            result => {
                result?;
                break;
            }
        }
    }
    let permission = config
        .feature_values
        .as_ref()
        .and_then(|features| features.get("permissionMode"))
        .and_then(Value::as_str);
    for (category, selected) in [
        ("thought_level", selection.effort.as_deref()),
        ("mode", config.mode_id.as_deref()),
        ("_permission", permission),
    ] {
        if let Some(value) = selected {
            select(transport, id, options, category, value).await?;
        }
    }
    Ok(())
}

/// Select native `value` for the option in `category` and verify the acknowledgement.
/// Unknown choices reject without a native request; `options` takes the confirmed state.
async fn select(
    transport: &mut Transport,
    id: &str,
    options: &mut Value,
    category: &str,
    value: &str,
) -> Result<(), AgentSessionError> {
    let selected_option = option(options, category).ok_or(AgentSessionError::Rejected)?;
    if !choices(selected_option)?
        .iter()
        .any(|choice| choice["value"] == *value)
    {
        return Err(AgentSessionError::Rejected);
    }
    if selected_option["currentValue"] == *value {
        return Ok(());
    }
    if category == "mode" {
        transport
            .request("session/set_mode", json!({"sessionId":id,"modeId":value}))
            .await?;
        return update_mode(options, value);
    }
    let result = transport
        .request(
            "session/set_config_option",
            json!({"sessionId":id,"configId":text(selected_option, "id")?,"value":value}),
        )
        .await?;
    *options = state(&result, Some(options))?;
    if option(options, category).is_none_or(|option| option["currentValue"] != *value) {
        return Err(AgentSessionError::Failed);
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
    let mut options = provider_native::acp_config::state(response)?;
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
