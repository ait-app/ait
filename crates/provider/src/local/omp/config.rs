use std::path::Path;

use domain::agent_runtime::StoredAgentConfig;
use serde_json::{Value, json};

use crate::ports::agent_session::{AgentSessionError, AgentSessionSpec};

pub(super) fn validate(config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
    if config.mode_id.as_deref().is_some_and(|mode| {
        !(matches!(mode, "full" | "write" | "ask" | "ask-cached")
            || matches!(mode, "default" | "plan"))
    }) || config
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
            key != "transport"
                && (!matches!(
                    key.as_str(),
                    "autoCompaction" | "autoRetry" | "fastMode" | "slowMode"
                ) || !value.is_boolean())
        })
    {
        return Err(AgentSessionError::Rejected);
    }
    if let Some(model) = &config.model {
        model_route(model)?;
    }
    validate_transport(config)?;
    Ok(())
}

/// Accept legacy protocol selections for migration, but never select an RPC connection.
/// Unknown or malformed selections reject before launching the native executable.
fn validate_transport(config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
    match config
        .feature_values
        .as_ref()
        .and_then(|values| values.get("transport"))
    {
        None => Ok(()),
        Some(Value::String(value)) if matches!(value.as_str(), "rpc" | "acp") => Ok(()),
        Some(_) => Err(AgentSessionError::Rejected),
    }
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

/// Native service-tier support for one selected model, discovered in a disposable session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ServiceTiers {
    pub(super) fast: bool,
    pub(super) slow: bool,
}

/// Return toggles for `config`, exposing only tiers confirmed by native `tiers` discovery.
pub(super) fn features(config: &StoredAgentConfig, tiers: ServiceTiers) -> Vec<Value> {
    [
        ("autoCompaction", "Automatic compaction"),
        ("autoRetry", "Automatic retry"),
        ("fastMode", "Fast service tier"),
        ("slowMode", "Slow service tier"),
    ]
    .into_iter()
    .filter(|(id, _)| match *id {
        "fastMode" => tiers.fast,
        "slowMode" => tiers.slow,
        _ => true,
    })
    .map(|(id, label)| {
        json!({"type":"toggle","id":id,"label":label,
            "value":config.feature_values.as_ref().and_then(|values|values.get(id))
                .and_then(Value::as_bool).unwrap_or(matches!(id,"autoCompaction"|"autoRetry"))})
    })
    .collect()
}

pub(super) fn model_thinking_options(model: &Value) -> Result<Vec<Value>, AgentSessionError> {
    let mut options = vec![json!({"id":"off","label":"off"})];
    if model["reasoning"] != true {
        return Ok(options);
    }
    let Some(efforts) = model["thinking"].get("efforts") else {
        return Ok(options);
    };
    let efforts = efforts
        .as_array()
        .filter(|efforts| efforts.len() <= 6)
        .ok_or(AgentSessionError::Failed)?;
    for effort in efforts {
        if !effort.as_str().is_some_and(|effort| {
            matches!(
                effort,
                "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
            )
        }) || options.iter().any(|option| option["id"] == *effort)
        {
            return Err(AgentSessionError::Failed);
        }
        options.push(json!({"id":effort,"label":effort}));
    }
    Ok(options)
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
                "kind":if entry["source"]=="skill" || entry["_meta"]["source"]=="skill"{"skill"}else{"command"}}),
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
