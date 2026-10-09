//! ACP launch configuration only; no project files or shared native configuration are written.
use std::{path::Path, process::Stdio, time::Duration};

use domain::agent_runtime::StoredAgentConfig;
use serde_json::{Value, json};
use tokio::{io::AsyncReadExt, process::Command};

use super::{OpenCodeClient, config};
use crate::{local::acp_transport::Transport, ports::agent_session::AgentSessionError};

/// Read and validate the installed CLI version; unsupported releases return unavailable.
pub(super) async fn version(
    client: &OpenCodeClient,
    cwd: &str,
) -> Result<String, AgentSessionError> {
    let output = read(client, cwd, &["--version"]).await?;
    let version = output
        .trim()
        .strip_prefix("opencode ")
        .unwrap_or(output.trim());
    let version = version.strip_prefix('v').unwrap_or(version);
    let numbers = version
        .split(['.', '-', '+'])
        .take(3)
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>();
    if !matches!(numbers.as_deref(), Ok([1 | 2, _, _])) {
        tracing::warn!("OpenCode ACP configuration format requires OpenCode 1.x or 2.x");
        return Err(AgentSessionError::Unavailable);
    }
    Ok(version.to_owned())
}

/// Initialize an owned ACP child in cwd; propagate launch and protocol failures.
pub(super) async fn spawn(
    client: &OpenCodeClient,
    cwd: &str,
    selected: &StoredAgentConfig,
) -> Result<(Transport, Value), AgentSessionError> {
    launch(client, cwd, selected, false).await
}

/// Initialize an owned tool-disabled ACP child; propagate configuration and launch failures.
pub(super) async fn auxiliary(
    client: &OpenCodeClient,
    cwd: &str,
    selected: &StoredAgentConfig,
) -> Result<(Transport, Value), AgentSessionError> {
    launch(client, cwd, selected, true).await
}

async fn launch(
    client: &OpenCodeClient,
    cwd: &str,
    selected: &StoredAgentConfig,
    auxiliary: bool,
) -> Result<(Transport, Value), AgentSessionError> {
    let version = version(client, cwd).await?;
    let legacy = version.starts_with("1.");
    let unsupported_forms = legacy
        || version
            .strip_prefix("2.0.")
            .and_then(|patch| patch.split(['-', '+']).next()?.parse::<u32>().ok())
            .is_some_and(|patch| patch < 26);
    let mut command = Command::new(&client.program);
    command.arg("acp").envs(&client.environment);
    if legacy {
        // Native 1.x excludes question in ACP. Do not enable an unanswerable native question.
        command.env("OPENCODE_ENABLE_QUESTION_TOOL", "false");
    }
    if auxiliary
        || (!legacy && unsupported_forms)
        || config::permission(selected).is_some()
        || selected.system_prompt.is_some()
    {
        let mut overlay: Value =
            serde_json::from_str(&overlay(client, selected, legacy, unsupported_forms)?)
                .map_err(|_| AgentSessionError::Rejected)?;
        if auxiliary {
            let agents = if legacy { "agent" } else { "agents" };
            let agent = native_agent(&mut overlay, agents, "build")?;
            agent.insert("steps".into(), json!(1));
            // The native permissions remove tools from an auxiliary model's tool catalog.
            if legacy {
                agent.insert("permission".into(), json!({"*":"deny"}));
            } else {
                agent.insert(
                    "permissions".into(),
                    json!([{"action":"*","resource":"*","effect":"deny"}]),
                );
            }
        }
        command.env("OPENCODE_CONFIG_CONTENT", overlay.to_string());
    }
    let mut transport = Transport::spawn(command, cwd, client.deadline)?;
    let initialized = transport
        .request(
            "initialize",
            json!({"protocolVersion":1,
        "clientCapabilities":{"elicitation":{"form":{}}},
        "clientInfo":{"name":"ait","version":env!("CARGO_PKG_VERSION")}}),
        )
        .await?;
    if initialized["protocolVersion"] != 1 {
        return Err(AgentSessionError::Failed);
    }
    let capabilities = &initialized["agentCapabilities"];
    *client
        .capabilities
        .write()
        .map_err(|_| AgentSessionError::Failed)? = Some(super::client::Capabilities {
        history: capabilities["loadSession"] == true,
        listing: capabilities["sessionCapabilities"]["list"].is_object(),
        deletion: capabilities["sessionCapabilities"]["delete"].is_object(),
    });
    Ok((transport, initialized["agentCapabilities"].clone()))
}

fn overlay(
    client: &OpenCodeClient,
    selected: &StoredAgentConfig,
    legacy: bool,
    unsupported_forms: bool,
) -> Result<String, AgentSessionError> {
    let inherited = client
        .environment
        .get("OPENCODE_CONFIG_CONTENT")
        .cloned()
        .or_else(|| std::env::var("OPENCODE_CONFIG_CONTENT").ok());
    let mut value: Value = inherited
        .map_or(Ok(json!({})), |text| serde_json::from_str(&text))
        .map_err(|_| AgentSessionError::Rejected)?;
    if !value.is_object() {
        return Err(AgentSessionError::Rejected);
    }
    if let Some(effect) = config::permission(selected) {
        if legacy {
            // A string is the native all-tools shorthand; retain native per-tool rules.
            if let Some(inherited) = value["permission"].as_str() {
                value["permission"] = json!({"*":inherited});
            }
            if value["permission"].is_null() {
                value["permission"] = json!({});
            }
            value["permission"]
                .as_object_mut()
                .ok_or(AgentSessionError::Rejected)?
                .insert("*".into(), json!(effect));
        } else {
            let rules = value
                .as_object_mut()
                .ok_or(AgentSessionError::Rejected)?
                .entry("permissions")
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .ok_or(AgentSessionError::Rejected)?;
            rules.push(json!({"action":"*","resource":"*","effect":effect}));
        }
    }
    if let Some(prompt) = &selected.system_prompt {
        let mode = selected.mode_id.as_deref().unwrap_or("build");
        let agents = if legacy { "agent" } else { "agents" };
        native_agent(&mut value, agents, mode)?.insert(
            if legacy { "prompt" } else { "system" }.into(),
            json!(prompt),
        );
    }
    if unsupported_forms && !legacy {
        let rules = value
            .as_object_mut()
            .ok_or(AgentSessionError::Rejected)?
            .entry("permissions")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or(AgentSessionError::Rejected)?;
        rules.push(json!({"action":"question","resource":"*","effect":"deny"}));
    }
    Ok(value.to_string())
}

fn native_agent<'a>(
    value: &'a mut Value,
    agents: &str,
    mode: &str,
) -> Result<&'a mut serde_json::Map<String, Value>, AgentSessionError> {
    value
        .as_object_mut()
        .ok_or(AgentSessionError::Rejected)?
        .entry(agents)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or(AgentSessionError::Rejected)?
        .entry(mode)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or(AgentSessionError::Rejected)
}

/// Bounded read-only CLI queries; stderr is never logged or returned.
pub(super) async fn read(
    client: &OpenCodeClient,
    cwd: &str,
    arguments: &[&str],
) -> Result<String, AgentSessionError> {
    if !Path::new(cwd).is_dir() {
        return Err(AgentSessionError::Rejected);
    }
    let mut command = Command::new(&client.program);
    command
        .args(arguments)
        .envs(&client.environment)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|error| {
        tracing::warn!(error_kind = ?error.kind(), os_error = error.raw_os_error(), "OpenCode CLI query failed to start");
        AgentSessionError::Unavailable
    })?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or(AgentSessionError::Failed)?
        .take(8 * 1024 * 1024 + 1);
    tokio::time::timeout(Duration::from_secs(30), async {
        let mut bytes = Vec::new();
        stdout
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| AgentSessionError::Failed)?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(AgentSessionError::Failed);
        }
        if !child
            .wait()
            .await
            .map_err(|_| AgentSessionError::Failed)?
            .success()
        {
            return Err(AgentSessionError::Failed);
        }
        String::from_utf8(bytes).map_err(|_| AgentSessionError::Failed)
    })
    .await
    .map_err(|_| AgentSessionError::Failed)?
}
