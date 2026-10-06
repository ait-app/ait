use std::path::Path;

use domain::agent_runtime::{StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};

use super::PROVIDER;
use crate::ports::agent_session::{AgentSessionError, AgentSessionSpec};

pub(super) fn validate(config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
    if config
        .mode_id
        .as_deref()
        .is_some_and(|mode| !matches!(mode, "default" | "accept-edits" | "plan" | "full-access"))
        || config.model.as_ref().is_some_and(|model| {
            model.is_empty() || model.len() > 512 || model.chars().any(char::is_control)
        })
        || config
            .thinking_option_id
            .as_deref()
            .is_some_and(|effort| !matches!(effort, "low" | "medium" | "high" | "xhigh" | "max"))
        || config.system_prompt.is_some()
        || config
            .tool_policy
            .as_ref()
            .is_some_and(|value| !value.is_null())
        || config
            .mcp_servers
            .as_ref()
            .is_some_and(|values| !values.is_empty())
        || config
            .provider_options
            .as_ref()
            .is_some_and(|values| !values.is_empty())
        || config
            .feature_values
            .as_ref()
            .is_some_and(|values| !values.is_empty())
    {
        return Err(AgentSessionError::Rejected);
    }
    Ok(())
}

pub(super) fn validate_directory(cwd: &str) -> Result<(), AgentSessionError> {
    if !Path::new(cwd).is_absolute() || !Path::new(cwd).is_dir() {
        return Err(AgentSessionError::Rejected);
    }
    Ok(())
}

pub(super) fn validate_spec(spec: &AgentSessionSpec) -> Result<(), AgentSessionError> {
    if spec.provider != PROVIDER {
        return Err(AgentSessionError::Rejected);
    }
    validate_directory(&spec.cwd)?;
    validate(&spec.config)
}

pub(super) fn arguments(config: &StoredAgentConfig, conversation: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = [
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--print-timeout",
        "0",
        "--disable-slash-commands",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for (flag, value) in [
        ("--conversation", conversation),
        ("--model", config.model.as_deref()),
        ("--effort", config.thinking_option_id.as_deref()),
    ] {
        if let Some(value) = value {
            args.extend([flag.to_owned(), value.to_owned()]);
        }
    }
    match config.mode_id.as_deref() {
        Some("plan" | "accept-edits") => {
            args.extend([
                "--mode".to_owned(),
                config.mode_id.clone().expect("matched mode"),
            ]);
        }
        Some("full-access") => args.push("--dangerously-skip-permissions".to_owned()),
        _ => {}
    }
    args
}

pub(super) fn runtime(
    id: &str,
    config: &StoredAgentConfig,
    init: &Value,
) -> StoredAgentRuntimeInfo {
    StoredAgentRuntimeInfo {
        provider: PROVIDER.to_owned(),
        session_id: Some(id.to_owned()),
        model: init["model"].as_str().map(str::to_owned),
        thinking_option_id: config.thinking_option_id.clone(),
        mode_id: Some(config.mode_id.as_deref().unwrap_or("default").to_owned()),
        extra: Some(std::collections::BTreeMap::from([(
            "permissionMode".to_owned(),
            init["permission_mode"].clone(),
        )])),
    }
}

pub(super) fn modes() -> Vec<Value> {
    vec![
        json!({
            "id":"default", "label":"Local Permissions",
            "description":concat!("Uses AGY's local permission rules. ",
                "Tools requiring interactive approval are denied in headless mode."),
            "icon":"Shield", "colorTier":"moderate",
        }),
        json!({
            "id":"accept-edits", "label":"Accept Edits",
            "description":"Uses AGY's accept-edits execution mode and local tool permission rules.",
            "icon":"ShieldPlus", "colorTier":"moderate",
        }),
        json!({
            "id":"plan", "label":"Plan",
            "description":"Uses AGY's planning execution mode.",
            "icon":"ShieldEllipsis", "colorTier":"planning",
        }),
        json!({
            "id":"full-access", "label":"Full Access",
            "description":"Approves all AGY tool calls, including commands and file writes.",
            "icon":"ShieldOff", "colorTier":"dangerous", "isUnattended":true,
        }),
    ]
}

#[cfg(test)]
mod tests;
