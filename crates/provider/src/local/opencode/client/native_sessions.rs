//! Discover and inspect provider-owned sessions without submitting input.
use std::{collections::HashSet, time::Duration};

use futures_util::{StreamExt, stream};
use reqwest::{Method, Url};

use super::{
    AgentPersistenceHandle, AgentSessionError, BTreeMap, CancellationToken, ListOptions,
    OpenCodeClient, Path, SessionDescriptor, SessionHistory, StoredAgentConfig, Value, error,
    history, invocation, json, runtime, session, validate_directory,
};
use crate::local::opencode::http::{Api, MAX_BODY, Version};

impl OpenCodeClient {
    pub(super) async fn list_native(
        &self,
        options: &ListOptions,
    ) -> Result<Vec<SessionDescriptor>, AgentSessionError> {
        if options.scan_limit == 0 || options.scan_limit > 4096 {
            return Err(AgentSessionError::Rejected);
        }
        let cwd = options.cwd.clone().map_or_else(
            || std::env::current_dir().map_err(|_| AgentSessionError::Failed),
            |cwd| Ok(cwd.into()),
        )?;
        let mut runtime =
            runtime::Runtime::spawn(&self.driver.binary, &cwd, &CancellationToken::new())
                .await
                .map_err(error)?;
        let result = tokio::time::timeout(Duration::from_secs(30), async {
            let mut sessions = list(&runtime.api, options).await?;
            previews(&runtime.api, &mut sessions).await;
            Ok(sessions)
        })
        .await
        .unwrap_or(Err(AgentSessionError::Failed));
        let _ = runtime.close().await;
        result
    }

    pub(super) async fn read_external(
        &self,
        handle: &AgentPersistenceHandle,
        cwd: &str,
    ) -> Result<SessionHistory, AgentSessionError> {
        validate_directory(cwd)?;
        let mut runtime = runtime::Runtime::spawn(
            &self.driver.binary,
            Path::new(cwd),
            &CancellationToken::new(),
        )
        .await
        .map_err(error)?;
        let result = inspect(&runtime.api, &handle.session_id, cwd).await;
        let _ = runtime.close().await;
        result
    }
}

async fn list(
    api: &Api,
    options: &ListOptions,
) -> Result<Vec<SessionDescriptor>, AgentSessionError> {
    let mut sessions = Vec::new();
    let mut cursors = HashSet::new();
    let mut cursor: Option<String> = None;
    let mut bytes = 0;
    loop {
        let mut url = Url::parse("http://127.0.0.1/").expect("constant loopback URL");
        url.set_path(match api.version {
            Version::V1 => "/experimental/session",
            Version::V2 => "/api/session",
        });
        {
            let mut query = url.query_pairs_mut();
            let remaining = options.scan_limit - sessions.len();
            let limit = if api.version == Version::V1 {
                remaining
            } else {
                remaining.min(100)
            };
            query.append_pair("limit", &limit.to_string());
            if let Some(cwd) = &options.cwd {
                query.append_pair("directory", cwd);
            }
            if api.version == Version::V2 {
                if let Some(cursor) = &cursor {
                    query.append_pair("cursor", cursor);
                } else {
                    query.append_pair("order", "desc");
                }
            }
        }
        let path = format!("{}?{}", url.path(), url.query().unwrap_or_default());
        let response = api.json(Method::GET, &path, None).await.map_err(error)?;
        bytes += response.to_string().len();
        if bytes > MAX_BODY {
            return Err(AgentSessionError::Failed);
        }
        let rows = api
            .data(&response)
            .as_array()
            .ok_or(AgentSessionError::Failed)?;
        for row in rows.iter().take(options.scan_limit - sessions.len()) {
            sessions.push(descriptor(api.version, row)?);
        }
        if api.version == Version::V1 || sessions.len() >= options.scan_limit {
            return Ok(sessions);
        }
        cursor = match response.pointer("/cursor/next") {
            Some(Value::String(next)) if !next.is_empty() => Some(next.clone()),
            Some(Value::Null) | None => return Ok(sessions),
            Some(_) => return Err(AgentSessionError::Failed),
        };
        if !cursors.insert(cursor.clone()) {
            return Err(AgentSessionError::Failed);
        }
    }
}

async fn previews(api: &Api, sessions: &mut [SessionDescriptor]) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let inputs: Vec<_> = sessions
        .iter()
        .enumerate()
        .map(|(index, session)| {
            (
                index,
                session.provider_handle_id.clone(),
                api.for_directory(&session.cwd),
            )
        })
        .collect();
    let mut reads = stream::iter(inputs)
        .map(|(index, id, api)| async move {
            let result = tokio::time::timeout(Duration::from_secs(2), api.history(&id)).await;
            (index, api.version, result)
        })
        .buffer_unordered(4);
    while let Ok(Some((index, version, result))) =
        tokio::time::timeout_at(deadline, reads.next()).await
    {
        let Ok(Ok(messages)) = result else {
            continue;
        };
        let mut prompts = messages
            .iter()
            .filter_map(|message| prompt(version, message));
        let session = &mut sessions[index];
        session.first_prompt_preview = prompts.next();
        session.last_prompt_preview = prompts
            .next_back()
            .or_else(|| session.first_prompt_preview.clone());
    }
}

fn prompt(version: Version, message: &Value) -> Option<String> {
    use crate::local::session_preview;
    match version {
        Version::V1 if message["info"]["role"] == "user" => session_preview::text(
            message["parts"]
                .as_array()?
                .iter()
                .filter(|part| {
                    part["type"] == "text" && part["synthetic"] != true && part["ignored"] != true
                })
                .filter_map(|part| part["text"].as_str()),
        ),
        Version::V2 if message["type"] == "user" => {
            session_preview::text(message["text"].as_str().into_iter())
        }
        Version::V1 | Version::V2 => None,
    }
}

fn descriptor(version: Version, info: &Value) -> Result<SessionDescriptor, AgentSessionError> {
    let id = text(info, "id")?;
    if !session::valid_id(id) {
        return Err(AgentSessionError::Rejected);
    }
    let cwd = match version {
        Version::V1 => text(info, "directory")?,
        Version::V2 => text(&info["location"], "directory")?,
    };
    if !Path::new(cwd).is_absolute() {
        return Err(AgentSessionError::Rejected);
    }
    Ok(SessionDescriptor {
        provider_id: "opencode".into(),
        provider_label: "OpenCode".into(),
        provider_handle_id: id.into(),
        cwd: cwd.into(),
        title: info["title"].as_str().map(str::to_owned),
        first_prompt_preview: None,
        last_prompt_preview: None,
        last_activity_at: timestamp(&info["time"]["updated"])?,
    })
}

async fn inspect(api: &Api, id: &str, cwd: &str) -> Result<SessionHistory, AgentSessionError> {
    let response = api
        .json(Method::GET, &api.path(id, ""), None)
        .await
        .map_err(error)?;
    let info = api.data(&response);
    let facts = descriptor(api.version, info)?;
    if facts.provider_handle_id != id || facts.cwd != cwd {
        return Err(AgentSessionError::Rejected);
    }
    let (model, agent) = match api.version {
        Version::V2 => (
            info["model"].clone(),
            info["agent"].as_str().unwrap_or("build").to_owned(),
        ),
        Version::V1 => {
            let messages = api.history(id).await.map_err(error)?;
            messages
                .iter()
                .rev()
                .find_map(|message| {
                    let info = &message["info"];
                    if info["role"] == "user" {
                        Some((
                            info["model"].clone(),
                            info["agent"].as_str().unwrap_or("build").to_owned(),
                        ))
                    } else {
                        None
                    }
                })
                .ok_or(AgentSessionError::Rejected)?
        }
    };
    let model_id = format!(
        "{}/{}",
        text(&model, "providerID")?,
        text(
            &model,
            match api.version {
                Version::V1 => "modelID",
                Version::V2 => "id",
            }
        )?
    );
    let config = StoredAgentConfig {
        mode_id: Some(agent),
        model: Some(model_id.clone()),
        thinking_option_id: model["variant"]
            .as_str()
            .filter(|variant| *variant != "default")
            .map(str::to_owned),
        ..Default::default()
    };
    let mut request = invocation(
        &super::AgentSessionSpec {
            provider: "opencode".into(),
            cwd: cwd.into(),
            config: config.clone(),
        },
        Some(id.into()),
    )?;
    request.verify_settings = false;
    let snapshot = session::snapshot(api, id, &request).await.map_err(error)?;
    let mut result = history(&snapshot, config.clone(), &BTreeMap::new())?;
    result.descriptor.title = facts.title;
    result.descriptor.last_activity_at = facts.last_activity_at;
    result.created_at = timestamp(&info["time"]["created"])?;
    result.parent_id = info["parentID"].as_str().map(str::to_owned);
    result.resume_metadata.insert(
        "opencode".into(),
        json!({
            "config": config, "model": model_id, "clients": {},
        }),
    );
    Ok(result)
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, AgentSessionError> {
    value[key]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or(AgentSessionError::Failed)
}

fn timestamp(value: &Value) -> Result<String, AgentSessionError> {
    value
        .as_i64()
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|value| value.to_rfc3339())
        .ok_or(AgentSessionError::Failed)
}

#[cfg(test)]
mod tests;
