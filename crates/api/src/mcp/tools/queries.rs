//! Compact, bounded read projections for external MCP callers.

use model::ErrorCode;
use serde_json::{Value, json};

use crate::Shared;
use crate::connection::rpc;

use super::creation;

/// Return whether a capability `value` reports a business rejection.
pub(super) fn failed(value: &Value) -> bool {
    value.get("error").is_some_and(|error| !error.is_null())
        || value["success"] == false
        || value["accepted"] == false
}

/// Return Agent status and snapshot selected by `arguments` in `state`.
/// # Errors
/// Propagates existing Agent lookup failures.
pub(super) async fn status(state: &Shared, arguments: Value) -> Result<Value, ErrorCode> {
    let value = rpc::execute(state, "agent.get.request", arguments).await?;
    if failed(&value) {
        return Ok(value);
    }
    Ok(json!({"status":value["agent"]["status"],"snapshot":value["agent"]}))
}

/// Return a compact Agent directory page filtered and bounded by `arguments` in `state`.
/// # Errors
/// Returns invalid filters, pagination or directory failures.
pub(super) async fn agents(state: &Shared, arguments: &Value) -> Result<Value, ErrorCode> {
    let mut params = json!({
        "filter":{"includeArchived":arguments["includeArchived"].as_bool().unwrap_or(false)},
        "sort":[{"key":"updated_at","direction":"desc"}],
        "page":{"limit":arguments["limit"].as_u64().unwrap_or(50)}
    });
    if let Some(statuses) = arguments.get("statuses") {
        params["filter"]["statuses"] = statuses.clone();
    }
    if let Some(cursor) = arguments.get("cursor") {
        params["page"]["cursor"] = cursor.clone();
    }
    let value = rpc::execute(state, "agent.list.request", params).await?;
    let entries = value["entries"]
        .as_array()
        .ok_or(ErrorCode::InvalidMessage)?;
    let agents: Vec<_> = entries.iter().map(|entry| {
        let agent = &entry["agent"];
        json!({"id":agent["id"],"title":agent["title"],"provider":agent["provider"],
            "cwd":agent["cwd"],"workspaceId":agent["workspaceId"],"status":agent["status"],
            "labels":agent["labels"],"updatedAt":agent["updatedAt"],"archivedAt":agent["archivedAt"],
            "pendingPermissions":agent["pendingPermissions"]})
    }).collect();
    Ok(json!({"agents":agents,"nextCursor":value["pageInfo"]["nextCursor"]}))
}

/// Return a Workspace page and next cursor selected by `arguments` from `state`.
/// # Errors
/// Propagates Workspace directory or pagination failures.
pub(super) async fn workspaces(state: &Shared, arguments: &Value) -> Result<Value, ErrorCode> {
    let mut params = json!({"page":{"limit":arguments["limit"].as_u64().unwrap_or(50)}});
    if let Some(cursor) = arguments.get("cursor") {
        params["page"]["cursor"] = cursor.clone();
    }
    let value = rpc::execute(state, "workspace.list.request", params).await?;
    Ok(json!({"workspaces":value["entries"],"nextCursor":value["pageInfo"]["nextCursor"]}))
}

/// Read all visible pending permissions through bounded directory pages in `state`.
/// # Errors
/// Returns existing directory or malformed result errors.
pub(super) async fn permissions(state: &Shared) -> Result<Value, ErrorCode> {
    let mut params = json!({"page":{"limit":200}});
    let mut permissions = Vec::new();
    loop {
        let value = rpc::execute(state, "agent.list.request", params.clone()).await?;
        let entries = value["entries"]
            .as_array()
            .ok_or(ErrorCode::InvalidMessage)?;
        for entry in entries {
            let agent = &entry["agent"];
            if let Some(pending) = agent["pendingPermissions"].as_array() {
                permissions.extend(
                    pending
                        .iter()
                        .map(|request| json!({"agentId":agent["id"],"request":request})),
                );
            }
        }
        if !value["pageInfo"]["nextCursor"].is_string() {
            break;
        }
        params["page"]["cursor"] = value["pageInfo"]["nextCursor"].clone();
    }
    Ok(json!({"permissions":permissions}))
}

/// Return saved daemon profiles from `state`, or an empty array when none are configured.
/// # Errors
/// Propagates existing daemon configuration failures.
pub(super) async fn profiles(state: &Shared) -> Result<Value, ErrorCode> {
    let value = rpc::execute(state, "daemon.config.get.request", json!({})).await?;
    let profiles = value["config"]["agentProfiles"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    Ok(json!({"profiles":profiles}))
}

/// Read the Agent's bounded recent timeline selected by `arguments` in `state`.
/// # Errors
/// Returns missing Agent, invalid page or native history failures.
pub(super) async fn activity(state: &Shared, arguments: Value) -> Result<Value, ErrorCode> {
    let agent = creation::require_success(state, &arguments["agentId"]).await?;
    let params = json!({"agentId":agent["id"],"direction":"tail",
        "limit":arguments["limit"].as_u64().unwrap_or(20)});
    let value = rpc::execute(state, "agent.timeline.get.request", params).await?;
    let items = value["entries"]
        .as_array()
        .or_else(|| value["items"].as_array())
        .ok_or(ErrorCode::InvalidMessage)?;
    let content = items
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    Ok(json!({"agentId":agent["id"],"updateCount":items.len(),
        "currentModeId":agent["currentModeId"],"content":content,"items":items}))
}

/// Inspect modes and draft features from `arguments`, with optional `caller` cwd in `state`.
/// # Errors
/// Returns invalid draft settings, missing cwd or existing discovery failures.
pub(super) async fn provider(
    state: &Shared,
    mut arguments: Value,
    caller: Option<&Value>,
) -> Result<Value, ErrorCode> {
    let (provider, model) = creation::provider_model(&arguments["provider"], false)?;
    let provider = provider.to_owned();
    let model = model.map(str::to_owned);
    creation::directory(state, &mut arguments, caller).await?;
    let available_modes = rpc::execute(
        state,
        "provider.modes.list.request",
        json!({"provider":provider,"cwd":arguments["cwd"]}),
    )
    .await?;
    let mut draft = arguments
        .get("settings")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if let Some(model) = model
        && draft.get("model").is_none()
    {
        draft["model"] = json!(model);
    }
    super::rename(&mut draft, "features", "featureValues")?;
    draft["provider"] = json!(provider);
    draft["cwd"] = arguments["cwd"].clone();
    let features = rpc::execute(
        state,
        "provider.features.list.request",
        json!({"draftConfig":draft}),
    )
    .await?;
    Ok(
        json!({"provider":provider,"modes":available_modes["modes"],"selectedModel":draft["model"],"features":features["features"]}),
    )
}
