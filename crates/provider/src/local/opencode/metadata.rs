//! Private, tool-disabled native requests; transient native history is removed on exit.
use std::{path::Path, time::Duration};

use reqwest::Method;
use serde_json::{Value, json};

use super::{http::Version, runtime::Runtime};
use crate::ports::agent_session::{AgentSessionError, AgentSessionSpec};

pub(super) fn configuration(version: Version, agent: &str) -> Value {
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

struct Request {
    runtime: Option<Runtime>,
    session: Option<String>,
}

impl Request {
    async fn close(&mut self) -> Result<(), AgentSessionError> {
        let Some(mut runtime) = self.runtime.take() else {
            return Ok(());
        };
        let result = if let Some(id) = self.session.take() {
            let _ = runtime
                .api
                .json(
                    Method::POST,
                    &runtime.api.path(&id, "/abort"),
                    Some(&json!({})),
                )
                .await;
            runtime
                .api
                .json(Method::DELETE, &runtime.api.path(&id, ""), None)
                .await
                .map(|_| ())
        } else {
            Ok(())
        };
        let closed = runtime.close().await;
        result.and(closed).map_err(|_| AgentSessionError::Failed)
    }
}

impl Drop for Request {
    fn drop(&mut self) {
        let Some(runtime) = self.runtime.take() else {
            return;
        };
        let session = self.session.take();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let mut request = Request {
                    runtime: Some(runtime),
                    session,
                };
                let _ = tokio::time::timeout(Duration::from_secs(5), request.close()).await;
            });
        }
    }
}

pub(super) async fn generate(
    binary: &Path,
    spec: &AgentSessionSpec,
    prompt: &str,
    schema: &Value,
) -> Result<String, AgentSessionError> {
    let (provider, model) = spec
        .config
        .model
        .as_deref()
        .and_then(|model| model.split_once('/'))
        .ok_or(AgentSessionError::Rejected)?;
    let agent = format!("ait-metadata-{}", uuid::Uuid::new_v4().simple());
    let runtime = Runtime::spawn_metadata(binary, Path::new(&spec.cwd), &agent)
        .await
        .map_err(|_| AgentSessionError::Unavailable)?;
    let mut request = Request {
        runtime: Some(runtime),
        session: None,
    };
    let result = run(
        &mut request,
        spec,
        &agent,
        (provider, model),
        &format!("{prompt}\nReturn only JSON matching this schema: {schema}"),
    )
    .await;
    let closed = request.close().await;
    closed?;
    result
}

async fn run(
    request: &mut Request,
    spec: &AgentSessionSpec,
    agent: &str,
    model: (&str, &str),
    prompt: &str,
) -> Result<String, AgentSessionError> {
    let api = &request
        .runtime
        .as_ref()
        .ok_or(AgentSessionError::Failed)?
        .api;
    let mut selected = match api.version {
        Version::V1 => json!({"providerID":model.0,"modelID":model.1}),
        Version::V2 => json!({"providerID":model.0,"id":model.1}),
    };
    if api.version == Version::V2
        && let Some(effort) = &spec.config.thinking_option_id
    {
        selected["variant"] = json!(effort);
    }
    let create = create_parameters(api.version, spec, agent, &selected);
    if api.version == Version::V2 {
        request.session = create["id"].as_str().map(str::to_owned);
    }
    let created = api
        .json(
            Method::POST,
            &format!("{}/session", api.version.prefix()),
            Some(&create),
        )
        .await
        .map_err(|_| AgentSessionError::Failed)?;
    let id = api.data(&created)["id"]
        .as_str()
        .filter(|id| super::session::valid_id(id))
        .ok_or(AgentSessionError::Failed)?
        .to_owned();
    request.session = Some(id.clone());
    let (suffix, mut body) = match api.version {
        Version::V1 => (
            "/prompt_async",
            json!({"agent":agent,"model":selected,"parts":[{"type":"text","text":prompt}],"tools":{"*":false}}),
        ),
        Version::V2 => ("/prompt", json!({"text":prompt,"files":[]})),
    };
    if api.version == Version::V1
        && let Some(effort) = &spec.config.thinking_option_id
    {
        body["variant"] = json!(effort);
    }
    api.json(Method::POST, &api.path(&id, suffix), Some(&body))
        .await
        .map_err(|_| AgentSessionError::Failed)?;
    loop {
        let history = api
            .history(&id)
            .await
            .map_err(|_| AgentSessionError::Failed)?;
        if api.idle(&id).await.map_err(|_| AgentSessionError::Failed)?
            && let Some(text) = response(api.version, &history)?
        {
            return Ok(text);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn create_parameters(
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

fn response(version: Version, history: &[Value]) -> Result<Option<String>, AgentSessionError> {
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
