//! Native catalogs own model names, modes and model-specific effort values.
use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::{PROVIDER, client::OpenCodeClient, config, launcher, summary::Temporary};
use provider_core::{ports::agent_session::AgentSessionError, protocol::provider::Details};
use provider_native::acp_config;

/// Query native options without a prompt, deleting the temporary session before returning.
/// Unavailable capabilities, malformed choices and cleanup failures are propagated.
pub(super) async fn discover(
    client: &OpenCodeClient,
    cwd: &str,
) -> Result<Details, AgentSessionError> {
    let (mut transport, capabilities, source) = launcher::observed(
        client,
        cwd,
        &domain::agent_runtime::StoredAgentConfig::default(),
    )
    .await?;
    if !capabilities["sessionCapabilities"]["delete"].is_object() {
        if let Some(source) = source {
            let result = async {
                let (providers, agents) = source.catalog(cwd, client.startup_timeout).await?;
                catalog::native(&providers, &agents)
            }
            .await;
            if let Ok(details) = result {
                transport.close().await?;
                return Ok(details);
            }
            tracing::debug!("OpenCode owned catalog unavailable; using read-only CLI queries");
        }
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
            .startup_request(
                "session/new",
                json!({"cwd":cwd,"mcpServers":[]}),
                client.startup_timeout,
            )
            .await?;
        let id = acp_config::text(&created, "sessionId")?.to_owned();
        temporary.id = Some(id.clone());
        let mut options = acp_config::state(&created)?;
        let model_option =
            acp_config::option(&options, "model").ok_or(AgentSessionError::Failed)?;
        let default = acp_config::text(model_option, "currentValue")?.to_owned();
        let mut models = BTreeMap::new();
        loop {
            let model_option =
                acp_config::option(&options, "model").ok_or(AgentSessionError::Failed)?;
            let choice = acp_config::choices(model_option)?
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
                acp_config::text(&choice, "value")?,
            )
            .await?;
            models.insert(
                acp_config::text(&choice, "value")?.to_owned(),
                model(&choice, &options, &default)?,
            );
        }
        // Native discovery can add or remove models while options are being inspected.
        // Publish the final catalog in native order, with efforts from each selected model.
        let model_option =
            acp_config::option(&options, "model").ok_or(AgentSessionError::Failed)?;
        let models = acp_config::choices(model_option)?
            .into_iter()
            .map(|choice| {
                let mut model = models
                    .remove(acp_config::text(choice, "value")?)
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
    let thinking = acp_config::option(options, "thought_level");
    let efforts = thinking
        .map(acp_config::choices)
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
