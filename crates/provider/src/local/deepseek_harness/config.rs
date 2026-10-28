use std::path::Path;

use domain::agent_runtime::{StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};

// The adapter's ACP configuration surface: shared helpers stay owned by `acp_config`,
// while this facade keeps adapter-local call sites on one path.
pub(super) use crate::local::acp_config::{choices, option, state, text};

use super::{PROVIDER, transport::Transport};
use crate::ports::agent_session::{AgentSessionError, AgentSessionSpec};
use crate::protocol::provider::Details;

pub(super) fn validate(config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
    if config.mode_id.is_some()
        || config.system_prompt.is_some()
        || config
            .tool_policy
            .as_ref()
            .is_some_and(|value| !value.is_null())
        || config
            .provider_options
            .as_ref()
            .is_some_and(|options| !options.is_empty())
        || config
            .feature_values
            .as_ref()
            .is_some_and(|features| !features.is_empty())
        || [&config.model, &config.thinking_option_id]
            .into_iter()
            .flatten()
            .any(|value| value.len() > 1024 || value.chars().any(char::is_control))
        || config.model.as_ref().is_some_and(String::is_empty)
    {
        return Err(AgentSessionError::Rejected);
    }
    crate::local::configuration::validate(config, PROVIDER)?;
    for (_, server) in config.mcp_servers.iter().flatten() {
        if server["type"] == "sse"
            || (server["type"] == "stdio"
                && !server["command"]
                    .as_str()
                    .is_some_and(|command| Path::new(command).is_absolute()))
        {
            return Err(AgentSessionError::Rejected);
        }
    }
    Ok(())
}

pub(super) fn validate_spec(spec: &AgentSessionSpec) -> Result<(), AgentSessionError> {
    if spec.provider != PROVIDER
        || !Path::new(&spec.cwd).is_absolute()
        || !Path::new(&spec.cwd).is_dir()
    {
        return Err(AgentSessionError::Rejected);
    }
    validate(&spec.config)
}

pub(super) fn details(options: &Value) -> Result<Details, AgentSessionError> {
    let Some(models) = option(options, "model") else {
        return Ok(Details::default());
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
                let mut model = json!({"provider":PROVIDER,
                    "id":choice["value"],"label":choice["name"],
                    "isDefault":choice["value"] == models["currentValue"],"isSelectable":true,
                    "thinkingOptions":thinking_options});
                if let Some(description) = choice["description"].as_str() {
                    model["description"] = json!(description);
                }
                if let Some(default) = thinking.and_then(|option| option["currentValue"].as_str()) {
                    model["defaultThinkingOptionId"] = json!(default);
                }
                model
            })
            .collect(),
        ..Details::default()
    })
}

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
        mode_id: None,
        extra: None,
    }
}

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
    ] {
        let Some(value) = selected else { continue };
        let option = option(options, category).ok_or(AgentSessionError::Rejected)?;
        if !choices(option)?
            .iter()
            .any(|choice| choice["value"] == *value)
        {
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
    }
    Ok(())
}

#[cfg(test)]
mod tests;
