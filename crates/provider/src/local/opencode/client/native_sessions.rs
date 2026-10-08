//! Discover and inspect provider-owned sessions without submitting input.
use std::{collections::HashSet, time::Duration};

use futures_util::{StreamExt, stream};
use reqwest::Method;

use super::{
    AgentPersistenceHandle, AgentSessionError, BTreeMap, CancellationToken, ListOptions,
    OpenCodeClient, Path, SessionDescriptor, SessionHistory, error, history, invocation, json,
    runtime, session, validate_directory,
};
use crate::local::opencode::protocol::{
    discovery::timestamp,
    http::{Api, MAX_BODY},
};

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
        let path = api.session_list_path(
            options.scan_limit - sessions.len(),
            options.cwd.as_deref(),
            cursor.as_deref(),
        );
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
            sessions.push(api.session_descriptor(row)?);
        }
        if sessions.len() >= options.scan_limit {
            return Ok(sessions);
        }
        cursor = api.session_list_cursor(&response)?;
        if cursor.is_none() {
            return Ok(sessions);
        }
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
            (index, api, result)
        })
        .buffer_unordered(4);
    while let Ok(Some((index, api, result))) = tokio::time::timeout_at(deadline, reads.next()).await
    {
        let Ok(Ok(messages)) = result else {
            continue;
        };
        let mut prompts = messages
            .iter()
            .filter_map(|message| api.prompt_preview(message));
        let session = &mut sessions[index];
        session.first_prompt_preview = prompts.next();
        session.last_prompt_preview = prompts
            .next_back()
            .or_else(|| session.first_prompt_preview.clone());
    }
}

async fn inspect(api: &Api, id: &str, cwd: &str) -> Result<SessionHistory, AgentSessionError> {
    let response = api
        .json(Method::GET, &api.path(id, ""), None)
        .await
        .map_err(error)?;
    let info = api.data(&response);
    let facts = api.session_descriptor(info)?;
    if facts.provider_handle_id != id || facts.cwd != cwd {
        return Err(AgentSessionError::Rejected);
    }
    let config = api.saved_config(info, id).await?;
    let model_id = config
        .model
        .as_deref()
        .ok_or(AgentSessionError::Failed)?
        .to_owned();
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

#[cfg(test)]
use crate::local::opencode::protocol::{
    Version,
    discovery::{descriptor, prompt},
};
#[cfg(test)]
mod tests;
