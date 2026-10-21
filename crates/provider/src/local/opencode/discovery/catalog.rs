//! Read-only native CLI discovery when ACP cannot delete a temporary query session.
use futures_util::{StreamExt, TryStreamExt, stream};
use serde_json::{Value, json};

use super::{Details, OpenCodeClient, PROVIDER, config, launcher};
use crate::ports::agent_session::AgentSessionError;

/// Query native models and primary agents without creating or modifying native sessions.
/// CLI failures and malformed or oversized catalogs are propagated.
pub(super) async fn discover(
    client: &OpenCodeClient,
    cwd: &str,
) -> Result<Details, AgentSessionError> {
    let models = async {
        let catalog = launcher::read(client, cwd, &["models", "--verbose"]).await?;
        parse_models(&catalog)
    };
    let (models, available_modes) = tokio::try_join!(models, modes(client, cwd))?;
    Ok(Details {
        models,
        modes: available_modes,
        features: config::features(&domain::agent_runtime::StoredAgentConfig::default()),
    })
}

async fn modes(client: &OpenCodeClient, cwd: &str) -> Result<Vec<Value>, AgentSessionError> {
    let agents = launcher::read(client, cwd, &["agent", "list"]).await?;
    Ok(stream::iter(parse_modes(&agents)?)
        .map(|mut mode| async move {
            let id = config::text(&mode, "id")?;
            let output = launcher::read(client, cwd, &["debug", "agent", id]).await?;
            let agent: Value =
                serde_json::from_str(&output).map_err(|_| AgentSessionError::Failed)?;
            if !agent.is_object() || agent["name"] != id {
                return Err(AgentSessionError::Failed);
            }
            if agent["hidden"] == true {
                return Ok(None);
            }
            if let Some(description) = agent.get("description") {
                mode["description"] = description.clone();
            }
            Ok(Some(mode))
        })
        .buffered(4)
        .try_collect::<Vec<_>>()
        .await?
        .into_iter()
        .flatten()
        .collect())
}

fn parse_models(output: &str) -> Result<Vec<Value>, AgentSessionError> {
    let mut remaining = output.trim();
    let mut models = Vec::new();
    while !remaining.is_empty() {
        let (id, metadata) = remaining
            .split_once('\n')
            .ok_or(AgentSessionError::Failed)?;
        let id = id.trim();
        let mut stream = serde_json::Deserializer::from_str(metadata).into_iter::<Value>();
        let model = stream
            .next()
            .ok_or(AgentSessionError::Failed)?
            .map_err(|_| AgentSessionError::Failed)?;
        if models.len() >= 4096 {
            return Err(AgentSessionError::Failed);
        }
        models.push(model_entry(id, &model)?);
        remaining = metadata[stream.byte_offset()..].trim_start();
    }
    Ok(models)
}

/// Project the verified legacy server's effective catalogs, retaining native choices and limits.
/// Malformed or oversized metadata fails so callers can use the read-only CLI fallback.
pub(super) fn native(providers: &Value, agents: &[Value]) -> Result<Details, AgentSessionError> {
    let providers = providers["providers"]
        .as_array()
        .ok_or(AgentSessionError::Failed)?;
    if providers.len() > 4096 || agents.len() > 128 {
        return Err(AgentSessionError::Failed);
    }
    let mut models = Vec::new();
    for provider in providers {
        let provider_id = config::text(provider, "id")?;
        let entries = provider["models"]
            .as_object()
            .ok_or(AgentSessionError::Failed)?;
        for (id, metadata) in entries {
            if models.len() >= 4096 {
                return Err(AgentSessionError::Failed);
            }
            models.push(model_entry(&format!("{provider_id}/{id}"), metadata)?);
        }
    }
    let mut available_modes = Vec::new();
    for agent in agents {
        let id = config::text(agent, "name")?;
        if agent["hidden"] == true || agent["mode"] == "subagent" {
            continue;
        }
        if !matches!(agent["mode"].as_str(), Some("primary" | "all")) {
            return Err(AgentSessionError::Failed);
        }
        validate_mode(id)?;
        let mut mode = mode_entry(id);
        if let Some(description) = agent.get("description") {
            mode["description"] = description.clone();
        }
        available_modes.push(mode);
    }
    if available_modes.is_empty() {
        return Err(AgentSessionError::Failed);
    }
    Ok(Details {
        models,
        modes: available_modes,
        features: config::features(&domain::agent_runtime::StoredAgentConfig::default()),
    })
}

fn model_entry(id: &str, metadata: &Value) -> Result<Value, AgentSessionError> {
    config::validate(&domain::agent_runtime::StoredAgentConfig {
        model: Some(id.into()),
        ..Default::default()
    })?;
    let name = config::text(metadata, "name")?;
    let variants = metadata["variants"].as_object();
    if variants.is_some_and(|variants| variants.len() > 128) {
        return Err(AgentSessionError::Failed);
    }
    let efforts = variants
        .into_iter()
        .flatten()
        .filter(|(_, value)| value["disabled"] != true)
        .map(|(id, _)| json!({"id":id,"label":id}))
        .collect::<Vec<_>>();
    Ok(json!({"provider":PROVIDER,"id":id,"label":name,
        "description":name,"isSelectable":true,"isDefault":false,
        "thinkingOptions":efforts}))
}

fn validate_mode(id: &str) -> Result<(), AgentSessionError> {
    config::validate(&domain::agent_runtime::StoredAgentConfig {
        mode_id: Some(id.into()),
        ..Default::default()
    })?;
    if id.starts_with('-') {
        return Err(AgentSessionError::Rejected);
    }
    Ok(())
}

fn mode_entry(id: &str) -> Value {
    json!({"id":id,"label":id,
        "icon":if id == "build" {"Hammer"} else if id == "plan" {"ShieldCheck"} else {"Bot"},
        "colorTier":if id == "plan" {"planning"} else {"moderate"}})
}

fn parse_modes(output: &str) -> Result<Vec<Value>, AgentSessionError> {
    let mut modes = Vec::new();
    for line in output.lines() {
        let Some(id) = line
            .strip_suffix(" (primary)")
            .or_else(|| line.strip_suffix(" (all)"))
        else {
            continue;
        };
        validate_mode(id)?;
        if modes.len() >= 128 {
            return Err(AgentSessionError::Failed);
        }
        modes.push(mode_entry(id));
    }
    if modes.is_empty() {
        return Err(AgentSessionError::Failed);
    }
    Ok(modes)
}

#[cfg(test)]
mod tests;
