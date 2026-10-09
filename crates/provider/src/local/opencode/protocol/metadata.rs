//! Tool-disabled auxiliary request codecs; cleanup uses the same native interrupt route.
use super::{Version, http::Api};
use crate::ports::agent_session::{AgentSessionError, AgentSessionSpec};
use reqwest::Method;
use serde_json::{Value, json};

pub(in crate::local::opencode) fn configuration(version: Version, agent: &str) -> Value {
    let instruction =
        "Generate only the requested metadata from the supplied text. Return only JSON.";
    match version {
        Version::V1 => json!({"agent":{agent:{"mode":"primary","steps":1,"prompt":instruction,
            "tools":{"*":false},"permission":{"*":"deny"}}},"permission":{"*":"deny"}}),
        Version::V2 => json!({"agents":{agent:{"mode":"primary","steps":1,"system":instruction,
            "permissions":[{"action":"*","resource":"*","effect":"deny"}]}},
            "permissions":[{"action":"*","resource":"*","effect":"deny"}]}),
    }
}

pub(in crate::local::opencode) struct MetadataRequest {
    pub id: Option<String>,
    pub create: Value,
    pub prompt: Value,
}

impl Version {
    pub(in crate::local::opencode) fn metadata_configuration(self, agent: &str) -> Value {
        configuration(self, agent)
    }
}

impl Api {
    pub(in crate::local::opencode) fn metadata_parameters(
        &self,
        spec: &AgentSessionSpec,
        agent: &str,
        model: (&str, &str),
        prompt: &str,
    ) -> MetadataRequest {
        let selected = self
            .version
            .model(model, spec.config.thinking_option_id.as_deref());
        let create = create_parameters(self.version, spec, agent, &selected);
        let mut body = match self.version {
            Version::V1 => {
                json!({"agent":agent,"model":selected,"parts":[{"type":"text","text":prompt}],"tools":{"*":false}})
            }
            Version::V2 => json!({"text":prompt,"files":[]}),
        };
        if self.version == Version::V1
            && let Some(effort) = &spec.config.thinking_option_id
        {
            body["variant"] = json!(effort);
        }
        MetadataRequest {
            id: create["id"].as_str().map(str::to_owned),
            create,
            prompt: body,
        }
    }

    pub(in crate::local::opencode) async fn create_native(
        &self,
        body: &Value,
    ) -> Result<Value, super::ProtocolError> {
        self.json(
            Method::POST,
            &format!("{}/session", self.version.prefix()),
            Some(body),
        )
        .await
    }

    pub(in crate::local::opencode) async fn submit_native(
        &self,
        id: &str,
        body: &Value,
    ) -> Result<(), super::ProtocolError> {
        self.json(
            Method::POST,
            &self.path(id, self.prompt_suffix()),
            Some(body),
        )
        .await?;
        Ok(())
    }

    pub(in crate::local::opencode) fn metadata_response(
        &self,
        history: &[Value],
    ) -> Result<Option<String>, AgentSessionError> {
        response(self.version, history)
    }
}

pub(in crate::local::opencode) fn create_parameters(
    version: Version,
    spec: &AgentSessionSpec,
    agent: &str,
    model: &Value,
) -> Value {
    match version {
        // V1 validates a different schema. The private server configuration denies tools;
        // agent/model/variant are selected on prompt_async, as in the foreground adapter.
        Version::V1 => json!({}),
        Version::V2 => json!({
            "id":format!("ses_{}", uuid::Uuid::new_v4().simple()),
            "agent":agent,"model":model,"location":{"directory":spec.cwd},
            "permission":[{"permission":"*","pattern":"*","action":"deny"}]
        }),
    }
}

pub(in crate::local::opencode) fn response(
    version: Version,
    history: &[Value],
) -> Result<Option<String>, AgentSessionError> {
    let mut text = String::new();
    for message in history {
        match version {
            Version::V1 if message["info"]["role"] == "assistant" => {
                if !message["info"]["error"].is_null() {
                    return Err(AgentSessionError::Failed);
                }
                if message
                    .pointer("/info/time/completed")
                    .is_none_or(Value::is_null)
                {
                    return Ok(None);
                }
                for part in message["parts"].as_array().into_iter().flatten() {
                    if part["type"] == "tool" {
                        return Err(AgentSessionError::Rejected);
                    }
                    if part["type"] == "text" {
                        text.push_str(part["text"].as_str().unwrap_or_default());
                    }
                }
            }
            Version::V2 if message["type"] == "assistant" => {
                if !message["error"].is_null() {
                    return Err(AgentSessionError::Failed);
                }
                if message
                    .pointer("/time/completed")
                    .is_none_or(Value::is_null)
                {
                    return Ok(None);
                }
                for part in message["content"].as_array().into_iter().flatten() {
                    if part["type"] == "tool" {
                        return Err(AgentSessionError::Rejected);
                    }
                    if part["type"] == "text" {
                        text.push_str(part["text"].as_str().unwrap_or_default());
                    }
                }
            }
            Version::V2 | Version::V1 => {}
        }
        if text.len() > 128 * 1024 {
            return Err(AgentSessionError::Failed);
        }
    }
    Ok((!text.is_empty()).then_some(text))
}

#[cfg(test)]
mod tests;
