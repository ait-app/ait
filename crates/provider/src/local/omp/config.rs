use std::path::Path;

use domain::agent_runtime::{StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};

use super::transport::Transport;
use crate::ports::agent_session::{AgentSessionError, AgentSessionSpec};
use crate::protocol::provider::Details;

pub(super) fn validate(config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
    if config
        .mode_id
        .as_deref()
        .is_some_and(|mode| !matches!(mode, "full" | "write" | "ask" | "ask-cached"))
        || config
            .provider_options
            .as_ref()
            .is_some_and(|values| !values.is_empty())
        || config
            .mcp_servers
            .as_ref()
            .is_some_and(|values| !values.is_empty())
        || config
            .tool_policy
            .as_ref()
            .is_some_and(|value| !value.is_null())
        || config
            .system_prompt
            .as_ref()
            .is_some_and(|value| value.len() > 65536 || value.contains('\0'))
        || config.thinking_option_id.as_deref().is_some_and(|value| {
            !matches!(
                value,
                "off" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
            )
        })
        || config.feature_values.iter().flatten().any(|(key, value)| {
            !matches!(
                key.as_str(),
                "autoCompaction" | "autoRetry" | "fastMode" | "slowMode"
            ) || !value.is_boolean()
        })
    {
        return Err(AgentSessionError::Rejected);
    }
    if let Some(model) = &config.model {
        model_route(model)?;
    }
    Ok(())
}

pub(super) fn validate_spec(spec: &AgentSessionSpec) -> Result<(), AgentSessionError> {
    if spec.provider != "omp"
        || !Path::new(&spec.cwd).is_absolute()
        || !Path::new(&spec.cwd).is_dir()
    {
        return Err(AgentSessionError::Rejected);
    }
    validate(&spec.config)
}

pub(super) fn model_route(id: &str) -> Result<[String; 2], AgentSessionError> {
    let route: [String; 2] = serde_json::from_str(id).map_err(|_| AgentSessionError::Rejected)?;
    if route
        .iter()
        .any(|part| part.is_empty() || part.len() > 1024 || part.chars().any(char::is_control))
    {
        return Err(AgentSessionError::Rejected);
    }
    Ok(route)
}

pub(super) fn features(config: &StoredAgentConfig) -> Vec<Value> {
    [
        ("autoCompaction", "Automatic compaction"),
        ("autoRetry", "Automatic retry"),
        ("fastMode", "Fast service tier"),
        ("slowMode", "Slow service tier"),
    ]
    .into_iter()
    .map(|(id, label)| {
        json!({"type":"toggle","id":id,"label":label,
            "value":config.feature_values.as_ref().and_then(|values|values.get(id))
                .and_then(Value::as_bool).unwrap_or(matches!(id,"autoCompaction"|"autoRetry"))})
    })
    .collect()
}

pub(super) async fn apply(
    transport: &mut Transport,
    config: &StoredAgentConfig,
) -> Result<Value, AgentSessionError> {
    validate(config)?;
    if let Some(model) = &config.model {
        let [provider, model] = model_route(model)?;
        transport
            .request("set_model", json!({"provider":provider,"modelId":model}))
            .await?;
    }
    if let Some(level) = &config.thinking_option_id {
        let levels = transport
            .request("get_available_thinking_levels", json!({}))
            .await?;
        if !levels["levels"]
            .as_array()
            .is_some_and(|levels| levels.iter().any(|item| item == level))
        {
            return Err(AgentSessionError::Rejected);
        }
        transport
            .request("set_thinking_level", json!({"level":level}))
            .await?;
    }
    for (id, value) in config.feature_values.iter().flatten() {
        let kind = match id.as_str() {
            "autoCompaction" => "set_auto_compaction",
            "autoRetry" => "set_auto_retry",
            "fastMode" => "set_fast_mode",
            "slowMode" => "set_slow_mode",
            _ => return Err(AgentSessionError::Rejected),
        };
        transport.request(kind, json!({"enabled":value})).await?;
    }
    transport.request("get_state", json!({})).await
}

pub(super) async fn discover(transport: &mut Transport) -> Result<Details, AgentSessionError> {
    let state = transport.request("get_state", json!({})).await?;
    let models = transport.request("get_available_models", json!({})).await?;
    let models = models["models"]
        .as_array()
        .filter(|models| models.len() <= 8192)
        .ok_or(AgentSessionError::Failed)?;
    let available = transport
        .request("get_available_thinking_levels", json!({}))
        .await?;
    let levels = available["levels"]
        .as_array()
        .ok_or(AgentSessionError::Failed)?;
    let mut entries = Vec::with_capacity(models.len());
    for model in models {
        let provider = text(model, "provider")?;
        let id = text(model, "id")?;
        let thinking: Vec<Value> =
            if state["model"]["provider"] == provider && state["model"]["id"] == id {
                levels
                    .iter()
                    .map(|level| json!({"id":level,"label":level}))
                    .collect()
            } else {
                Vec::new()
            };
        entries.push(
            json!({"provider":"omp","id":json!([provider,id]).to_string(),
            "label":model["name"].as_str().unwrap_or(id),"isSelectable":true,
            "isDefault":state["model"]["provider"] == provider && state["model"]["id"] == id,
            "thinkingOptions":thinking}),
        );
    }
    Ok(Details {
        models: entries,
        modes: modes(),
        features: features(&StoredAgentConfig::default()),
    })
}

pub(super) fn runtime(state: &Value) -> Result<StoredAgentRuntimeInfo, AgentSessionError> {
    let id = text(state, "sessionId")?;
    let model = state
        .get("model")
        .filter(|model| !model.is_null())
        .map(|model| Ok(json!([text(model, "provider")?, text(model, "id")?]).to_string()))
        .transpose()?;
    Ok(StoredAgentRuntimeInfo {
        provider: "omp".to_owned(),
        session_id: Some(id.to_owned()),
        model,
        thinking_option_id: state["thinkingLevel"].as_str().map(str::to_owned),
        mode_id: None,
        extra: None,
    })
}

pub(super) fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, AgentSessionError> {
    value[key]
        .as_str()
        .filter(|text| {
            !text.is_empty() && text.len() <= 4096 && !text.chars().any(char::is_control)
        })
        .ok_or(AgentSessionError::Failed)
}

pub(super) fn commands(value: &Value) -> Result<Vec<Value>, AgentSessionError> {
    value["commands"]
        .as_array()
        .filter(|commands| commands.len() <= 4096)
        .ok_or(AgentSessionError::Failed)?
        .iter()
        .map(|entry| {
            Ok(
                json!({"name":text(entry,"name")?,"description":entry["description"].as_str()
                .unwrap_or_default(),"argumentHint":entry["input"]["hint"].as_str().unwrap_or_default(),
                "kind":if entry["source"]=="skill"{"skill"}else{"command"}}),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests;

pub(super) fn modes() -> Vec<Value> {
    [
        (
            "full",
            "Full Access",
            "Approves ordinary read, write and execution tools.",
            "dangerous",
        ),
        (
            "write",
            "Allow File Writes",
            "Allows reads and file writes; command execution requires approval.",
            "moderate",
        ),
        (
            "ask",
            "Always Ask",
            "Requires approval for write and execution tools.",
            "safe",
        ),
        (
            "ask-cached",
            "Ask and Remember",
            "Requires approval for ungranted files and execution tools.",
            "safe",
        ),
    ]
    .into_iter()
    .map(|(id, label, description, tier)| {
        json!({"id":id,"label":label,"description":description,
            "colorTier":tier,"isUnattended":id=="full"})
    })
    .collect()
}

pub(super) fn approval_mode(mode: &str) -> &str {
    match mode {
        "full" => "yolo",
        "ask" => "always-ask",
        "ask-cached" => "always-ask-cached",
        _ => "write",
    }
}
