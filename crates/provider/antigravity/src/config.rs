use std::path::Path;

use domain::agent_runtime::StoredAgentConfig;
use serde_json::{Value, json};

use super::{PROVIDER, discovery::EFFORTS};
use provider_core::ports::agent_session::{AgentSessionError, AgentSessionSpec};

/// Accept only settings AGY can receive natively; MCP servers are limited to Streamable HTTP,
/// the one transport confirmed against `agy acp` (stdio and SSE stay AGY-configured).
pub(super) fn validate(config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
    if config.mode_id.as_deref().is_some_and(|mode| {
        !matches!(
            mode,
            "default" | "accept-edits" | "auto" | "plan" | "full-access"
        )
    }) || config.model.as_ref().is_some_and(|model| {
        model.is_empty() || model.len() > 512 || model.chars().any(char::is_control)
    }) || config
        .thinking_option_id
        .as_deref()
        .is_some_and(|effort| !EFFORTS.contains(&effort))
        || config.system_prompt.is_some()
        || config
            .tool_policy
            .as_ref()
            .is_some_and(|value| !value.is_null())
        || config
            .mcp_servers
            .iter()
            .flatten()
            .any(|(_, server)| server["type"] != "http")
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
    provider_native::configuration::validate(config, PROVIDER)
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
            "description":concat!("Allows edits under AGY's local permission rules. ",
                "Commands requiring approval are denied in headless mode; ",
                "add a scoped AGY allow rule or select Full Access."),
            "icon":"ShieldPlus", "colorTier":"moderate",
        }),
        json!({
            "id":"auto", "label":"Auto",
            "description":concat!("Uses AGY's auto mode: edits and commands AGY judges safe run ",
                "without asking; irreversible actions still need approval and are denied in ",
                "headless mode."),
            "icon":"ShieldCheck", "colorTier":"moderate",
        }),
        json!({
            "id":"plan", "label":"Plan",
            "description":concat!("Uses AGY's planning execution mode. ",
                "Tools requiring interactive approval are denied in headless mode."),
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
