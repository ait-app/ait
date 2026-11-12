//! Shared ACP configuration helpers for stdio ACP adapters.

use serde_json::Value;

use provider_core::ports::agent_session::AgentSessionError;

/// Borrow the native select option for a protocol category, when advertised.
///
/// # Arguments
///
/// * `options` - Native `configOptions` snapshot.
/// * `category` - Protocol category such as `model` or `thought_level`.
///
/// # Returns
/// The select option whose category matches, or none when the adapter omits it.
pub(crate) fn option<'a>(options: &'a Value, category: &str) -> Option<&'a Value> {
    options
        .as_array()?
        .iter()
        .find(|option| option["category"] == category && option["type"] == "select")
}

/// Flatten native select groups within catalog bounds; malformed choices fail closed.
///
/// # Arguments
///
/// * `option` - One native select option.
///
/// # Returns
/// Every validated choice, with grouped choices flattened in declaration order.
///
/// # Errors
///
/// Returns `AgentSessionError::Failed` for a missing, oversized or malformed choice list.
pub(crate) fn choices(option: &Value) -> Result<Vec<&Value>, AgentSessionError> {
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

/// Validate and copy a bounded native configuration response, rejecting malformed options.
///
/// # Arguments
///
/// * `response` - Native response carrying a `configOptions` array.
///
/// # Returns
/// The validated options as a JSON array safe to retain as session state.
///
/// # Errors
///
/// Returns `AgentSessionError::Failed` for a missing, oversized or malformed option list.
pub(crate) fn state(response: &Value) -> Result<Value, AgentSessionError> {
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

/// Borrow a nonempty native string field; missing, oversized or control text is a failure.
///
/// # Arguments
///
/// * `value` - Native value owning the field.
/// * `key` - Field name to read.
///
/// # Returns
/// The borrowed text when it is nonempty, at most 1024 bytes and free of control characters.
///
/// # Errors
///
/// Returns `AgentSessionError::Failed` when the field is absent or out of bounds.
pub(crate) fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, AgentSessionError> {
    value[key]
        .as_str()
        .filter(|text| {
            !text.is_empty() && text.len() <= 1024 && !text.chars().any(char::is_control)
        })
        .ok_or(AgentSessionError::Failed)
}

/// Convert validated host MCP definitions to ACP launch descriptions.
/// Reads `config.mcp_servers`; returns an empty list when none are configured.
pub(crate) fn mcp_servers(config: &domain::agent_runtime::StoredAgentConfig) -> Vec<Value> {
    config
        .mcp_servers
        .iter()
        .flatten()
        .map(|(name, server)| {
            let pairs = |field: &str| -> Vec<Value> {
                server[field]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .map(|(name, value)| serde_json::json!({"name":name,"value":value}))
                    .collect()
            };
            if server["type"] == "stdio" {
                serde_json::json!({
                    "name":name,"command":server["command"],
                    "args":server.get("args").cloned().unwrap_or_else(|| serde_json::json!([])),
                    "env":pairs("env")
                })
            } else {
                serde_json::json!({
                    "type":server["type"],"name":name,
                    "url":server["url"],"headers":pairs("headers")
                })
            }
        })
        .collect()
}

/// MCP servers a configuration launches; an empty map launches none, like an absent one.
///
/// # Arguments
///
/// * `config` - Effective session configuration.
///
/// # Returns
/// The configured servers, or `None` when there are none.
pub(crate) fn launched_servers(
    config: &domain::agent_runtime::StoredAgentConfig,
) -> Option<&std::collections::BTreeMap<String, Value>> {
    config
        .mcp_servers
        .as_ref()
        .filter(|servers| !servers.is_empty())
}

/// Whether a validated host MCP definition is launchable without a native search path.
///
/// # Arguments
///
/// * `server` - One `mcp_servers` entry already accepted by shared configuration validation.
///
/// # Returns
/// True for Streamable HTTP servers and for stdio servers with an absolute command; false for
/// SSE, which the supported native clients reject, and for commands that need `PATH` lookup.
pub(crate) fn launchable(server: &Value) -> bool {
    server["type"] == "http"
        || (server["type"] == "stdio"
            && server["command"]
                .as_str()
                .is_some_and(|command| std::path::Path::new(command).is_absolute()))
}
