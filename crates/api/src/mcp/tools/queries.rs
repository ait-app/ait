//! Compact, bounded read projections for external MCP callers.

use std::collections::BTreeSet;

use bot::recall::Sight;
use model::ErrorCode;
use serde_json::{Value, json};

use crate::Shared;
use crate::connection::rpc;

use super::overview::AGENT_PAGE;
use super::{creation, recall};

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

/// Return a compact Agent directory page filtered and bounded by `arguments` in `state`,
/// limited to `workspaces` when given and to the Agents `sight` reaches (ADR-280 §4).
///
/// A sight that reaches only its own work reads the newest [`AGENT_PAGE`] Agents, keeps those it
/// reaches up to `limit`, and reports `truncated` instead of a cursor: the directory's cursor
/// encodes the id and update time of the last Agent it read, which may be someone else's.
/// # Errors
/// Returns invalid filters, pagination or directory failures.
pub(super) async fn agents(
    state: &Shared,
    arguments: &Value,
    workspaces: Option<&BTreeSet<String>>,
    sight: Option<&Sight>,
) -> Result<Value, ErrorCode> {
    let limit = arguments["limit"].as_u64().unwrap_or(50);
    let restricted = recall::external(sight);
    let mut params = json!({
        "filter":{"includeArchived":arguments["includeArchived"].as_bool().unwrap_or(false)},
        "sort":[{"key":"updated_at","direction":"desc"}],
        "page":{"limit":if restricted { AGENT_PAGE } else { limit }}
    });
    if let Some(statuses) = arguments.get("statuses") {
        params["filter"]["statuses"] = statuses.clone();
    }
    if let Some(cursor) = arguments.get("cursor") {
        params["page"]["cursor"] = cursor.clone();
    }
    within(&mut params, workspaces);
    let value = rpc::execute(state, "agent.list.request", params).await?;
    let entries = value["entries"]
        .as_array()
        .ok_or(ErrorCode::InvalidMessage)?;
    let mut agents: Vec<_> = entries.iter().map(|entry| &entry["agent"])
        .filter(|agent| recall::sees_agent(sight, agent)).map(|agent| {
        json!({"id":agent["id"],"title":agent["title"],"provider":agent["provider"],
            "cwd":agent["cwd"],"workspaceId":agent["workspaceId"],"status":agent["status"],
            "labels":agent["labels"],"updatedAt":agent["updatedAt"],"archivedAt":agent["archivedAt"],
            "pendingPermissions":agent["pendingPermissions"]})
    }).collect();
    let next = &value["pageInfo"]["nextCursor"];
    if !restricted {
        return Ok(json!({"agents":agents,"nextCursor":next}));
    }
    let kept = usize::try_from(limit).unwrap_or(usize::MAX);
    let truncated = agents.len() > kept || next.is_string();
    agents.truncate(kept);
    Ok(json!({"agents":agents,"nextCursor":null,"truncated":truncated}))
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

/// Read all visible pending permissions through bounded directory pages in `state`, limited to
/// Agents in `workspaces` when given and to the Agents `sight` reaches (ADR-280 §4); entries
/// carry only Agent ids, so they are filtered at the source.
/// # Errors
/// Returns existing directory or malformed result errors.
pub(super) async fn permissions(
    state: &Shared,
    workspaces: Option<&BTreeSet<String>>,
    sight: Option<&Sight>,
) -> Result<Value, ErrorCode> {
    let mut params = json!({"page":{"limit":200}});
    within(&mut params, workspaces);
    let mut permissions = Vec::new();
    loop {
        let value = rpc::execute(state, "agent.list.request", params.clone()).await?;
        let entries = value["entries"]
            .as_array()
            .ok_or(ErrorCode::InvalidMessage)?;
        let reached = entries
            .iter()
            .map(|entry| &entry["agent"])
            .filter(|agent| recall::sees_agent(sight, agent));
        for agent in reached {
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

/// Restrict directory `params` to `workspaces` when given.
fn within(params: &mut Value, workspaces: Option<&BTreeSet<String>>) {
    if let Some(workspaces) = workspaces {
        params["filter"]["workspaceIds"] = json!(workspaces);
    }
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

/// Read the Agent's bounded recent timeline selected by `arguments` in `state`, with the
/// Agent's status and last change read just before it.
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
    Ok(
        json!({"agentId":agent["id"],"status":agent["status"],"updatedAt":agent["updatedAt"],
        "updateCount":items.len(),"currentModeId":agent["currentModeId"],"content":content,
        "items":items}),
    )
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
