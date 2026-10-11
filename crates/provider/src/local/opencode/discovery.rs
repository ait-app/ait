//! ACP owns model names, mode choices and model-specific effort values.
use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::{OpenCodeClient, PROVIDER, config, launcher, summary::Temporary};
use crate::{ports::agent_session::AgentSessionError, protocol::provider::Details};

/// Query native options without a prompt, deleting the temporary session before returning.
/// Unavailable capabilities, malformed choices and cleanup failures are propagated.
pub(super) async fn discover(
    client: &OpenCodeClient,
    cwd: &str,
) -> Result<Details, AgentSessionError> {
    let (mut transport, capabilities) = launcher::spawn(
        client,
        cwd,
        &domain::agent_runtime::StoredAgentConfig::default(),
    )
    .await?;
    if !capabilities["sessionCapabilities"]["delete"].is_object() {
        transport.close().await?;
        if !launcher::version(client, cwd).await?.starts_with("1.") {
            return Err(AgentSessionError::Unavailable);
        }
        return catalog::discover(client, cwd).await;
    }
    let mut temporary = Temporary::new(client, cwd, transport);
    let result = async {
        let transport = temporary
            .transport
            .as_mut()
            .ok_or(AgentSessionError::Failed)?;
        let created = transport
            .request("session/new", json!({"cwd":cwd,"mcpServers":[]}))
            .await?;
        let id = config::text(&created, "sessionId")?.to_owned();
        temporary.id = Some(id.clone());
        let mut options = config::state(&created)?;
        let model_option = config::option(&options, "model").ok_or(AgentSessionError::Failed)?;
        let default = config::text(model_option, "currentValue")?.to_owned();
        let mut models = BTreeMap::new();
        loop {
            let model_option =
                config::option(&options, "model").ok_or(AgentSessionError::Failed)?;
            let choice = config::choices(model_option)?
                .into_iter()
                .find(|choice| {
                    choice["value"]
                        .as_str()
                        .is_some_and(|id| !models.contains_key(id))
                })
                .cloned();
            let Some(choice) = choice else { break };
            if models.len() >= 4096 {
                return Err(AgentSessionError::Failed);
            }
            config::select(
                transport,
                &id,
                &mut options,
                "model",
                config::text(&choice, "value")?,
            )
            .await?;
            models.insert(
                config::text(&choice, "value")?.to_owned(),
                model(&choice, &options, &default)?,
            );
        }
        // Native discovery can add or remove models while options are being inspected.
        // Publish the final catalog in native order, with efforts from each selected model.
        let model_option = config::option(&options, "model").ok_or(AgentSessionError::Failed)?;
        let models = config::choices(model_option)?
            .into_iter()
            .map(|choice| {
                let mut model = models
                    .remove(config::text(choice, "value")?)
                    .ok_or(AgentSessionError::Failed)?;
                model["label"] = choice["name"].clone();
                model["description"] = choice["description"].clone();
                Ok(model)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Details {
            models,
            modes: config::modes(&options),
            features: config::features(&domain::agent_runtime::StoredAgentConfig::default()),
        })
    }
    .await;
    temporary.close().await?;
    result
}

fn model(choice: &Value, options: &Value, default: &str) -> Result<Value, AgentSessionError> {
    let thinking = config::option(options, "thought_level");
    let efforts = thinking
        .map(config::choices)
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .map(|effort| {
            json!({"id":effort["value"],"label":effort["name"],
                "isDefault":thinking.is_some_and(|option| option["currentValue"] == effort["value"])})
        })
        .collect::<Vec<_>>();
    let mut model = json!({"provider":PROVIDER,"id":choice["value"],"label":choice["name"],
        "description":choice["description"],"isSelectable":true,"isDefault":choice["value"] == default,
        "thinkingOptions":efforts});
    if let Some(default) = thinking.and_then(|option| option["currentValue"].as_str()) {
        model["defaultThinkingOptionId"] = json!(default);
    }
    Ok(model)
}

mod catalog;

#[cfg(test)]
mod tests;
