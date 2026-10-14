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
    let catalog = launcher::read(client, cwd, &["models", "--verbose"]).await?;
    let agents = launcher::read(client, cwd, &["agent", "list"]).await?;
    let modes = stream::iter(parse_modes(&agents)?)
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
        .collect();
    Ok(Details {
        models: parse_models(&catalog)?,
        modes,
        features: config::features(&domain::agent_runtime::StoredAgentConfig::default()),
    })
}

fn parse_models(output: &str) -> Result<Vec<Value>, AgentSessionError> {
    let mut remaining = output.trim();
    let mut models = Vec::new();
    while !remaining.is_empty() {
        let (id, metadata) = remaining
            .split_once('\n')
            .ok_or(AgentSessionError::Failed)?;
        let id = id.trim();
        config::validate(&domain::agent_runtime::StoredAgentConfig {
            model: Some(id.into()),
            ..Default::default()
        })?;
        let mut stream = serde_json::Deserializer::from_str(metadata).into_iter::<Value>();
        let model = stream
            .next()
            .ok_or(AgentSessionError::Failed)?
            .map_err(|_| AgentSessionError::Failed)?;
        let name = config::text(&model, "name")?;
        let variants = model["variants"].as_object();
        if models.len() >= 4096 || variants.is_some_and(|variants| variants.len() > 128) {
            return Err(AgentSessionError::Failed);
        }
        let efforts = variants
            .into_iter()
            .flatten()
            .filter(|(_, value)| value["disabled"] != true)
            .map(|(id, _)| json!({"id":id,"label":id}))
            .collect::<Vec<_>>();
        models.push(json!({"provider":PROVIDER,"id":id,"label":name,
            "description":name,"isSelectable":true,"isDefault":false,
            "thinkingOptions":efforts}));
        remaining = metadata[stream.byte_offset()..].trim_start();
    }
    Ok(models)
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
        config::validate(&domain::agent_runtime::StoredAgentConfig {
            mode_id: Some(id.into()),
            ..Default::default()
        })?;
        if id.starts_with('-') {
            return Err(AgentSessionError::Rejected);
        }
        if modes.len() >= 128 {
            return Err(AgentSessionError::Failed);
        }
        modes.push(json!({"id":id,"label":id,
            "icon":if id == "build" {"Hammer"} else if id == "plan" {"ShieldCheck"} else {"Bot"},
            "colorTier":if id == "plan" {"planning"} else {"moderate"}}));
    }
    if modes.is_empty() {
        return Err(AgentSessionError::Failed);
    }
    Ok(modes)
}

#[cfg(test)]
mod tests;
