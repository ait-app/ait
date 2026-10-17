use std::{collections::BTreeMap, path::Path};

use domain::agent_runtime::{StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};

// The adapter's ACP configuration surface: shared helpers stay owned by `acp_config`,
// while this facade keeps adapter-local call sites on one path.
pub(super) use crate::local::acp_config::{choices, option, state, text};

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

/// Apply explicit selected values, ignoring a thought level the selected model no longer offers.
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
        if category == "thought_level" && option(options, category).is_none() {
            continue;
        }
        select(transport, id, options, category, value).await?;
    }
    Ok(())
}

/// Select a native value and await its confirmed state, including asynchronous option updates.
/// Unavailable choices reject before mutation; timeout or inconsistent protocol fails the connection.
pub(super) async fn select(
    transport: &mut Transport,
    id: &str,
    options: &mut Value,
    category: &str,
    value: &str,
) -> Result<(), AgentSessionError> {
    let native = option(options, category).ok_or_else(|| {
        tracing::warn!(
            category,
            "OpenCode ACP does not advertise this configuration option"
        );
        AgentSessionError::Rejected
    })?;
    if !choices(native)?
        .iter()
        .any(|choice| choice["value"] == value)
    {
        tracing::warn!(
            category,
            "OpenCode ACP does not advertise the requested configuration value"
        );
        return Err(AgentSessionError::Rejected);
    }
    if native["currentValue"] == value {
        return Ok(());
    }
    let params = json!({"sessionId":id,"configId":text(native, "id")?,"value":value});
    let deadline = tokio::time::Instant::now() + transport.control_deadline();
    let mut confirmed = None;
    let result = transport
        .request_with_updates("session/set_config_option", params, |message| {
            if let Some(update) = confirmation(message, id, category, value)? {
                confirmed = Some(update);
            }
            Ok(())
        })
        .await?;
    *options = state(&result)?;
    if let Some(confirmed) = confirmed {
        *options = confirmed;
        return Ok(());
    }
    if option(options, category).is_some_and(|native| native["currentValue"] == value) {
        return Ok(());
    }
    // Native model.updated can race the set response. Await the confirmation without
    // repeating the mutation or submitting a prompt against the previous model.
    let result = tokio::time::timeout_at(deadline, async {
        loop {
            let message = transport.receive().await?;
            if let Some(confirmed) = confirmation(&message, id, category, value)? {
                *options = confirmed;
                return Ok(());
            }
            if message.get("id").is_some() {
                return Err(AgentSessionError::Failed);
            }
        }
    })
    .await;
    if let Ok(Ok(())) = result {
        return Ok(());
    }
    transport.close().await?;
    Err(AgentSessionError::Failed)
}

fn confirmation(
    message: &Value,
    id: &str,
    category: &str,
    value: &str,
) -> Result<Option<Value>, AgentSessionError> {
    if message["method"] != "session/update"
        || message["params"]["update"]["sessionUpdate"] != "config_option_update"
    {
        return Ok(None);
    }
    if message["params"]["sessionId"] != id {
        return Err(AgentSessionError::Failed);
    }
    let options = state(&message["params"]["update"])?;
    Ok(option(&options, category)
        .is_some_and(|native| native["currentValue"] == value)
        .then_some(options))
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
        .into_iter().map(|choice| json!({"id":choice["value"],"label":choice["name"],"description":choice["description"],
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

#[cfg(test)]
mod tests;
