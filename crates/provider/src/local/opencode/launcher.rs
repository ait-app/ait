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
    if !matches!(numbers.as_deref(), Ok([2, minor, patch]) if *minor > 0 || *patch >= 26) {
        tracing::warn!("OpenCode ACP requires OpenCode 2.0.26 or newer for native question forms");
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
    version(client, cwd).await?;
    let mut command = Command::new(&client.program);
    command.arg("acp").envs(&client.environment);
    if auxiliary || config::permission(selected).is_some() || selected.system_prompt.is_some() {
        let mut overlay: Value =
            serde_json::from_str(&overlay(selected)?).map_err(|_| AgentSessionError::Rejected)?;
        if auxiliary {
            if overlay["agents"].is_null() {
                overlay["agents"] = json!({});
            }
            if overlay["agents"]["build"].is_null() {
                overlay["agents"]["build"] = json!({});
            }
            overlay["agents"]["build"]["steps"] = json!(1);
            // The native permissions remove tools from an auxiliary model's tool catalog.
            overlay["agents"]["build"]["permissions"] =
                json!([{"action":"*","resource":"*","effect":"deny"}]);
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
    Ok((transport, initialized["agentCapabilities"].clone()))
}

fn overlay(selected: &StoredAgentConfig) -> Result<String, AgentSessionError> {
    let mut value: Value = std::env::var("OPENCODE_CONFIG_CONTENT")
        .map_or(Ok(json!({})), |text| serde_json::from_str(&text))
        .map_err(|_| AgentSessionError::Rejected)?;
    if !value.is_object() {
        return Err(AgentSessionError::Rejected);
    }
    if let Some(effect) = config::permission(selected) {
        let rules = value
            .as_object_mut()
            .ok_or(AgentSessionError::Rejected)?
            .entry("permissions")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or(AgentSessionError::Rejected)?;
        rules.push(json!({"action":"*","resource":"*","effect":effect}));
    }
    if let Some(prompt) = &selected.system_prompt {
        let mode = selected.mode_id.as_deref().unwrap_or("build");
        if value["agents"].is_null() {
            value["agents"] = json!({});
        }
        if value["agents"][mode].is_null() {
            value["agents"][mode] = json!({});
        }
        value["agents"][mode]
            .as_object_mut()
            .ok_or(AgentSessionError::Rejected)?
            .insert("system".into(), json!(prompt));
    }
    Ok(value.to_string())
}

/// Bounded read-only CLI queries; output never includes stderr or native credentials.
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
