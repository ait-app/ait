//! ACP owns model names, mode choices and model-specific effort values.
use serde_json::{Value, json};

use super::{OpenCodeClient, PROVIDER, config, launcher, summary::Temporary};
use crate::{ports::agent_session::AgentSessionError, protocol::provider::Details};

/// Query native options without a prompt, deleting the temporary session before returning.
/// Unavailable capabilities, malformed choices and cleanup failures are propagated.
pub(super) async fn discover(
    client: &OpenCodeClient,
    cwd: &str,
) -> Result<Details, AgentSessionError> {
    let (transport, capabilities) = launcher::spawn(
        client,
        cwd,
        &domain::agent_runtime::StoredAgentConfig::default(),
    )
    .await?;
    if !capabilities["sessionCapabilities"]["delete"].is_object() {
        return Err(AgentSessionError::Unavailable);
    }
    let mut temporary = Temporary::new(client, cwd, transport);
    let result = async {
        let transport = temporary.transport.as_mut().ok_or(AgentSessionError::Failed)?;
        let created = transport.request("session/new", json!({"cwd":cwd,"mcpServers":[]})).await?;
        let id = config::text(&created, "sessionId")?.to_owned();
        temporary.id = Some(id.clone());
        let mut options = config::state(&created)?;
        let available_agents = config::option(&options, "mode").map(config::choices).transpose()?.unwrap_or_default().into_iter().map(|choice| {
            json!({"id":choice["value"],"label":choice["name"],"description":choice["description"],
                "icon":if choice["value"] == "build" {"Hammer"} else if choice["value"] == "plan" {"ShieldCheck"} else {"Bot"},
                "colorTier":if choice["value"] == "plan" {"planning"} else {"moderate"}})
        }).collect();
        let model_option = config::option(&options, "model").ok_or(AgentSessionError::Failed)?;
        let default = config::text(model_option, "currentValue")?.to_owned();
        let config_id = config::text(model_option, "id")?.to_owned();
        let choices: Vec<Value> = config::choices(model_option)?.into_iter().cloned().collect();
        let mut models = Vec::with_capacity(choices.len());
        for choice in choices {
            if config::option(&options, "model").is_none_or(|option| option["currentValue"] != choice["value"]) {
                let result = transport.request_with_updates("session/set_config_option", json!({"sessionId":id,"configId":config_id,"value":choice["value"]}), |_| Ok(())).await?;
                options = config::state(&result)?;
            }
            let thinking = config::option(&options, "thought_level");
            let efforts = thinking.map(config::choices).transpose()?.unwrap_or_default().into_iter()
                .map(|effort| json!({"id":effort["value"],"label":effort["name"],"isDefault":thinking.is_some_and(|option| option["currentValue"] == effort["value"])})).collect::<Vec<_>>();
            models.push(json!({"provider":PROVIDER,"id":choice["value"],"label":choice["name"],
                "description":choice["description"],"isSelectable":true,"isDefault":choice["value"] == default,
                "thinkingOptions":efforts,"defaultThinkingOptionId":thinking.and_then(|option| option["currentValue"].as_str())}));
        }
        Ok(Details { models, modes: available_agents, features: config::features(&domain::agent_runtime::StoredAgentConfig::default()) })
    }.await;
    temporary.close().await?;
    result
}
