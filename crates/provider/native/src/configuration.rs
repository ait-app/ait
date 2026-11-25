//! Shared, bounded configuration validation before invoking native providers.

mod options;

use std::collections::BTreeMap;

use domain::agent_runtime::StoredAgentConfig;
use serde_json::{Map, Value, json};

use provider_core::ports::agent_session::AgentSessionError;

/// Validate advanced settings without exposing their contents in errors.
///
/// # Errors
///
/// Returns [`AgentSessionError::Rejected`] when the settings are oversized or not supported by
/// `provider`.
pub fn validate(config: &StoredAgentConfig, provider: &str) -> Result<(), AgentSessionError> {
    let encoded = serde_json::to_vec(config).map_err(|_| AgentSessionError::Rejected)?;
    if encoded.len() > 256 * 1024
        || config
            .system_prompt
            .as_ref()
            .is_some_and(|text| !text_value(text))
    {
        return Err(AgentSessionError::Rejected);
    }
    // An empty map is the same as no options; providers without option rules accept it.
    if let Some(options) = config
        .provider_options
        .as_ref()
        .filter(|options| !options.is_empty())
    {
        options::validate(options, provider)?;
    }
    if let Some(servers) = &config.mcp_servers {
        if servers.len() > 128 {
            return Err(AgentSessionError::Rejected);
        }
        for (name, server) in servers {
            if !identifier(name) || !mcp_server(server) {
                return Err(AgentSessionError::Rejected);
            }
        }
    }
    grants(config)?;
    Ok(())
}

fn text_value(text: &str) -> bool {
    text.len() <= 65536 && !text.contains('\0')
}

fn identifier(text: &str) -> bool {
    !text.trim().is_empty() && text.len() <= 256 && text.chars().all(|value| !value.is_control())
}

fn string_map(value: &Value) -> bool {
    value.as_object().is_some_and(|values| {
        values.len() <= 256
            && values
                .iter()
                .all(|(key, value)| identifier(key) && value.as_str().is_some_and(text_value))
    })
}

fn strings(value: &Value) -> bool {
    value.as_array().is_some_and(|values| {
        values.len() <= 1024
            && values
                .iter()
                .all(|value| value.as_str().is_some_and(text_value))
    })
}

fn mcp_server(value: &Value) -> bool {
    let Some(fields) = value.as_object() else {
        return false;
    };
    let stdio = value["type"] == "stdio";
    if !stdio && value["type"] != "http" && value["type"] != "sse" {
        return false;
    }
    let required = if stdio { "command" } else { "url" };
    if !value[required].as_str().is_some_and(|text| {
        identifier(text) && (stdio || text.starts_with("https://") || text.starts_with("http://"))
    }) {
        return false;
    }
    fields.iter().all(|(key, value)| match key.as_str() {
        "type" => true,
        "command" if stdio => value.as_str().is_some_and(identifier),
        "url" if !stdio => value.as_str().is_some_and(identifier),
        "args" if stdio => strings(value),
        "env" if stdio => string_map(value),
        "headers" if !stdio => string_map(value),
        "alwaysLoad" => value.is_boolean(),
        _ => false,
    })
}

/// Exact MCP grants, grouped deterministically and deduplicated per server.
pub(super) fn grants(
    config: &StoredAgentConfig,
) -> Result<BTreeMap<&str, Vec<&str>>, AgentSessionError> {
    let mut grants: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let Some(policy) = config
        .tool_policy
        .as_ref()
        .filter(|policy| !policy.is_null())
    else {
        return Ok(grants);
    };
    let fields = policy.as_object().ok_or(AgentSessionError::Rejected)?;
    let entries = policy["preapproved"]
        .as_array()
        .filter(|entries| entries.len() <= 1024)
        .ok_or(AgentSessionError::Rejected)?;
    if fields
        .iter()
        .any(|(key, value)| key != "preapproved" && (key != "mcp" || !mcp_scope(value)))
    {
        return Err(AgentSessionError::Rejected);
    }
    for entry in entries {
        let fields = entry.as_object().ok_or(AgentSessionError::Rejected)?;
        let exact_name = |key: &str| {
            entry[key].as_str().filter(|name| {
                identifier(name)
                    && !name.contains(['*', '?', '[', ']', ',', '(', ')'])
                    && !name.contains("__")
                    && name.trim() == *name
            })
        };
        let server = exact_name("server").ok_or(AgentSessionError::Rejected)?;
        let tool = exact_name("tool").ok_or(AgentSessionError::Rejected)?;
        if fields.len() != 3 || entry["kind"] != "mcp" {
            return Err(AgentSessionError::Rejected);
        }
        let tools = grants.entry(server).or_default();
        if !tools.contains(&tool) {
            tools.push(tool);
        }
    }
    Ok(grants)
}

/// Whether `value` is a valid `toolPolicy.mcp`: `{"inherited":"none"}`, with an optional `keep`
/// list naming inherited servers the session may still use.
fn mcp_scope(value: &Value) -> bool {
    value.as_object().is_some_and(|fields| {
        fields.iter().all(|(key, value)| match key.as_str() {
            "inherited" => value == "none",
            "keep" => value.as_array().is_some_and(|names| {
                names.len() <= 128
                    && names
                        .iter()
                        .all(|name| name.as_str().is_some_and(identifier))
            }),
            _ => false,
        }) && fields.contains_key("inherited")
    })
}

/// Map options and transports to Codex's thread configuration overrides.
///
/// # Errors
///
/// Returns the same errors as [`validate`].
pub fn codex(config: &StoredAgentConfig) -> Result<Value, AgentSessionError> {
    validate(config, "codex")?;
    let mut result: Map<String, Value> = config
        .provider_options
        .clone()
        .unwrap_or_default()
        .into_iter()
        .collect();
    let mut servers = Map::new();
    codex_computer_use(config, &mut result);
    if let Some(approval) = result.get_mut("approval_policy") {
        complete_codex_approval(approval);
    }
    for (name, server) in config.mcp_servers.iter().flatten() {
        let mut native = Map::new();
        for (source, target) in [
            ("command", "command"),
            ("args", "args"),
            ("env", "env"),
            ("url", "url"),
            ("headers", "http_headers"),
        ] {
            if let Some(value) = server.get(source) {
                native.insert(target.to_owned(), value.clone());
            }
        }
        servers.insert(name.clone(), Value::Object(native));
    }
    for (server, tools) in grants(config)? {
        let native = servers
            .entry(server.to_owned())
            .or_insert_with(|| json!({}));
        native["enabled_tools"] = json!(tools);
        native["default_tools_approval_mode"] = json!("prompt");
        native["tools"] = Value::Object(
            tools
                .into_iter()
                .map(|tool| (tool.to_owned(), json!({"approval_mode":"approve"})))
                .collect(),
        );
    }
    if !servers.is_empty() {
        result.insert("mcp_servers".to_owned(), Value::Object(servers));
    }
    if let Some(effort) = &config.thinking_option_id {
        result.insert("model_reasoning_effort".to_owned(), json!(effort));
    }
    Ok(Value::Object(result))
}

/// Tools that reach every MCP server's resources whatever its per-server rules say.
const CLAUDE_RESOURCE_TOOLS: [&str; 2] = ["ListMcpResourcesTool", "ReadMcpResourceTool"];

/// The session's `toolPolicy.mcp`, when it limits the MCP servers it inherits.
fn scope(config: &StoredAgentConfig) -> Option<&Value> {
    config
        .tool_policy
        .as_ref()
        .and_then(|policy| policy.get("mcp"))
        .filter(|scope| scope["inherited"] == "none")
}

/// Inherited MCP servers a strict session keeps by name (`toolPolicy.mcp.keep`), such as the
/// user's own connection to the service a session reports back to.
#[must_use]
pub fn strict_keep(config: &StoredAgentConfig) -> Vec<&str> {
    scope(config)
        .and_then(|scope| scope.get("keep"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

/// The MCP servers an effective Codex configuration (`config/read`) loads, as
/// `{name, url, type, product}`; disabled servers are left out and the product is never known.
#[must_use]
pub fn codex_mcp_servers(effective: &Value) -> Vec<Value> {
    effective["config"]["mcp_servers"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, server)| server["enabled"] != false)
        .map(|(name, server)| {
            let url = server["url"].as_str().filter(|url| !url.is_empty());
            let kind = if server["command"].is_string() {
                "stdio"
            } else {
                "http"
            };
            json!({"name": name, "url": url, "type": kind, "product": null})
        })
        .collect()
}

/// Whether a strict Claude session keeps inherited servers by name. Claude has no strict mode
/// that keeps one, so such a session loads the user's configuration without claude.ai
/// connectors (`ENABLE_CLAUDEAI_MCP_SERVERS=false`: their list can arrive after start), denies
/// every other server by name in `disallowedTools` and is checked once it runs
/// (`claude_mcp_check`).
#[must_use]
pub fn claude_keeps(config: &StoredAgentConfig) -> bool {
    strict_mcp(config) && !strict_keep(config).is_empty()
}

/// The `disallowedTools` prefix that denies every tool of a Claude MCP server.
#[must_use]
pub fn claude_server_rule(name: &str) -> String {
    let name: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("mcp__{name}")
}

/// Check a running Claude session that keeps inherited servers against its `mcp_status`: each
/// loaded server must be the session's own, a kept one, or denied by name.
///
/// # Errors
///
/// Returns [`AgentSessionError::Rejected`] when a server could be used that is none of these.
pub fn claude_mcp_check(
    config: &StoredAgentConfig,
    status: &Value,
) -> Result<(), AgentSessionError> {
    let keep = strict_keep(config);
    let own = config.mcp_servers.as_ref();
    let denied: Vec<&str> = config
        .provider_options
        .as_ref()
        .and_then(|options| options.get("disallowedTools"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let servers = status["mcpServers"]
        .as_array()
        .ok_or(AgentSessionError::Rejected)?;
    for server in servers {
        let name = server["name"].as_str().ok_or(AgentSessionError::Rejected)?;
        let allowed = keep.contains(&name)
            || own.is_some_and(|own| own.contains_key(name))
            || denied.contains(&claude_server_rule(name).as_str());
        if !allowed {
            return Err(AgentSessionError::Rejected);
        }
    }
    Ok(())
}

/// Whether the session inherits no MCP servers beyond the ones it keeps by name
/// (`toolPolicy.mcp.inherited: "none"`).
#[must_use]
pub fn strict_mcp(config: &StoredAgentConfig) -> bool {
    scope(config).is_some()
}

/// Disable, for one Codex thread, every MCP server and plugin the user's effective configuration
/// (`config/read`) contributes, keeping only the session's own servers. Codex merges thread
/// overrides into the user's configuration, so servers can only be switched off by name.
///
/// # Errors
///
/// Returns [`AgentSessionError::Rejected`] when the user's configuration already has a server
/// named like one of the session's: Codex would merge its keys (headers, tokens, a command)
/// into the session's server, and a merge cannot remove them.
pub fn codex_strict(
    native: &mut Value,
    effective: &Value,
    config: &StoredAgentConfig,
) -> Result<(), AgentSessionError> {
    let own = config.mcp_servers.as_ref();
    let keep = strict_keep(config);
    let inherited = effective["config"]["mcp_servers"].as_object();
    for name in inherited.into_iter().flat_map(Map::keys) {
        if own.is_some_and(|own| own.contains_key(name)) {
            return Err(AgentSessionError::Rejected);
        }
        if keep.contains(&name.as_str()) {
            continue;
        }
        native["mcp_servers"][name] = json!({"enabled": false});
    }
    let plugins = effective["config"]["plugins"].as_object();
    for (plugin, state) in plugins.into_iter().flatten() {
        if state["enabled"] != false {
            native["plugins"][plugin] = json!({"enabled": false});
        }
    }
    // ChatGPT apps reach the thread through Codex's built-in `codex_apps` server; plugins
    // that `config/read` does not list are switched off as a whole.
    native["features"]["apps"] = json!(false);
    native["features"]["plugins"] = json!(false);
    Ok(())
}

/// Whether a permission preset permits automatic computer use approval.
/// Explicit per-tool settings override Auto-review and Full Access defaults.
pub fn codex_computer_use_auto_approval(config: &StoredAgentConfig) -> bool {
    matches!(
        config.mode_id.as_deref(),
        Some("auto-review" | "full-access")
    ) && config
        .provider_options
        .as_ref()
        .and_then(|options| options.get("plugins"))
        .and_then(|plugins| {
            plugins.pointer(
                "/unified-computer-use@openai-bundled/mcp_servers/cua_repl/tools/js/approval_mode",
            )
        })
        .and_then(Value::as_str)
        .is_none_or(|mode| mode == "approve")
}

/// Preapprove the bundled CUA tool for Auto-review and Full Access sessions.
/// Explicit per-tool settings take precedence over the preset.
fn codex_computer_use(config: &StoredAgentConfig, result: &mut Map<String, Value>) {
    if !codex_computer_use_auto_approval(config) {
        return;
    }
    let mut parent = result.entry("plugins").or_insert_with(|| json!({}));
    for key in [
        "unified-computer-use@openai-bundled",
        "mcp_servers",
        "cua_repl",
        "tools",
        "js",
    ] {
        parent = parent
            .as_object_mut()
            .expect("validated Codex plugin settings are objects")
            .entry(key)
            .or_insert_with(|| json!({}));
    }
    parent
        .as_object_mut()
        .expect("validated Codex tool settings are objects")
        .entry("approval_mode")
        .or_insert_with(|| json!("approve"));
}

/// Native JSON-RPC requires all three granular switches, unlike the optional config schema.
pub fn complete_codex_approval(approval: &mut Value) {
    if let Some(granular) = approval.get_mut("granular").and_then(Value::as_object_mut) {
        for field in ["rules", "sandbox_approval", "mcp_elicitations"] {
            granular
                .entry(field.to_owned())
                .or_insert(Value::Bool(false));
        }
    }
}

/// Native Claude CLI argument pairs, without a shell or interpolation.
///
/// A `confined` process already runs under the Workspace's OS write sandbox, which refuses
/// nested Seatbelt profiles or user namespaces, so Claude's own Bash sandbox is turned off.
///
/// # Errors
///
/// Returns the same errors as [`validate`].
pub fn claude(
    config: &StoredAgentConfig,
    confined: bool,
) -> Result<Vec<String>, AgentSessionError> {
    claude_with_environment(config, confined, &[])
}

/// Build the native Claude argument list with adapter-owned session environment entries.
///
/// `environment` arrives already mapped to Claude Code's own variable names by the adapter that
/// owns that vocabulary, so this shared crate never learns a provider's role set. The entries are
/// emitted through `--settings` rather than the process environment: verified on 2.1.325, the CLI
/// `--settings` document takes precedence over the user's `~/.claude/settings.json`, which is what
/// lets one session redirect a role without editing the user's configuration.
///
/// # Errors
///
/// Returns the same errors as [`validate`].
pub fn claude_with_environment(
    config: &StoredAgentConfig,
    confined: bool,
    environment: &[(&str, String)],
) -> Result<Vec<String>, AgentSessionError> {
    validate(config, "claude")?;
    let empty = BTreeMap::new();
    let options = config.provider_options.as_ref().unwrap_or(&empty);
    let mut args = Vec::new();
    let mut allowed = options
        .get("allowedTools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (server, tools) in grants(config)? {
        for tool in tools {
            let name = json!(format!("mcp__{server}__{tool}"));
            if !allowed.contains(&name) {
                allowed.push(name);
            }
        }
    }
    let mut denied = options
        .get("disallowedTools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if claude_keeps(config) {
        for tool in CLAUDE_RESOURCE_TOOLS {
            if !denied.contains(&json!(tool)) {
                denied.push(json!(tool));
            }
        }
    }
    for (flag, values) in [
        ("--allowedTools", Some(&allowed)),
        ("--disallowedTools", Some(&denied)),
    ] {
        if let Some(values) = values.filter(|values| !values.is_empty()) {
            args.push(format!(
                "{flag}={}",
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
    }
    for directory in options
        .get("additionalDirectories")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(directory) = directory.as_str() {
            args.push(format!("--add-dir={directory}"));
        }
    }
    let mut settings = options
        .get("settings")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if let Some(sandbox) = options.get("sandbox") {
        merge(&mut settings["sandbox"], sandbox);
    }
    if confined {
        merge(&mut settings["sandbox"], &json!({"enabled": false}));
    }
    if let Some(fast) = config
        .feature_values
        .as_ref()
        .and_then(|values| values.get("fast_mode"))
    {
        settings["fastMode"] = fast.clone();
    }
    if config.thinking_option_id.as_deref() == Some("ultracode") {
        settings["ultracode"] = json!(true);
    }
    for (variable, value) in environment {
        settings["env"][*variable] = json!(value);
    }
    if settings
        .as_object()
        .is_some_and(|settings| !settings.is_empty())
    {
        args.push(format!("--settings={settings}"));
    }
    if strict_mcp(config) && !claude_keeps(config) {
        // Only this session's servers: no user, project, plugin or claude.ai connector servers.
        args.push("--strict-mcp-config".to_owned());
    }
    Ok(args)
}

/// Native Claude MCP document; callers pass it through a private file because server headers
/// and environment may carry credentials that must not appear in process arguments.
#[must_use]
pub fn claude_mcp(config: &StoredAgentConfig) -> Option<Value> {
    config
        .mcp_servers
        .as_ref()
        .filter(|servers| !servers.is_empty())
        .map(|servers| json!({"mcpServers":servers}))
}

fn merge(target: &mut Value, source: &Value) {
    if let (Some(target), Some(source)) = (target.as_object_mut(), source.as_object()) {
        for (key, value) in source {
            merge(target.entry(key.clone()).or_insert(Value::Null), value);
        }
    } else {
        *target = source.clone();
    }
}

#[cfg(test)]
mod tests;

/// Whether `program` names a runnable file, directly or through PATH lookup.
#[must_use]
pub fn executable(program: &std::path::Path) -> bool {
    let runnable = |path: &std::path::Path| {
        let Ok(metadata) = path.metadata() else {
            return false;
        };
        if !metadata.is_file() {
            return false;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode() & 0o111 != 0
        }
        #[cfg(not(unix))]
        {
            true
        }
    };
    if program.components().count() > 1 {
        return runnable(program);
    }
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|path| {
            let path = path.join(program);
            #[cfg(windows)]
            let path = path.with_extension("exe");
            runnable(&path)
        })
    })
}
