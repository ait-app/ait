//! Agent methods: only Agents of the guest's workspace, never the daemon MCP.

use provider::service::agent_mcp::{GUEST_SHARE_LABEL, GUEST_USER_LABEL};
use serde_json::{Value, json};

use super::{ErrorCode, Rule, Session, conceal, object, present, terminal};
use crate::Shared;

/// Agent creation options that could place an Agent or its tools outside the workspace.
const CREATE_DENIED: [&str; 4] = ["worktree", "worktreeName", "git", "env"];

/// Native configuration a guest must not choose.
const CONFIG_DENIED: [&str; 2] = ["mcpServers", "providerOptions"];

/// Label namespace reserved for guest attribution.
const GUEST_LABEL_PREFIX: &str = "ait.guest.";

/// Most Agents or terminals one guest request may name.
const MAX_IDS: usize = 64;

pub(super) async fn confine(
    session: &Session,
    state: &Shared,
    rule: Rule,
    params: &mut Value,
) -> Result<(), ErrorCode> {
    match rule {
        Rule::AgentCreate => create(session, state, params).await,
        Rule::Agent(key) => scoped(session, state, params, key).await,
        Rule::AgentDrive(key) => driven(session, state, params, key).await,
        Rule::AgentUpdate => {
            if present(params.get("labels")) {
                return Err(ErrorCode::Forbidden);
            }
            driven(session, state, params, "agentId").await
        }
        Rule::AgentCommands => {
            if present(params.get("draftConfig")) {
                return Err(ErrorCode::Forbidden);
            }
            scoped(session, state, params, "agentId").await
        }
        Rule::AgentFilter => filter(session, params),
        Rule::AgentSubscription => many(session, state, params, "agentIds", false).await,
        Rule::ItemsClose => {
            many(session, state, params, "agentIds", true).await?;
            terminals(session, state, params).await
        }
        Rule::Open
        | Rule::Catalog
        | Rule::Workspace
        | Rule::WorkspaceList
        | Rule::TerminalCreate
        | Rule::TerminalFilter
        | Rule::Terminal
        | Rule::Path
        | Rule::Download
        | Rule::Deny => Err(ErrorCode::Forbidden),
    }
}

/// Resolve `identifier` like the provider (full ID, unique prefix, exact title) and return
/// the full ID when the Agent belongs to the guest's workspace.
async fn resolve(session: &Session, state: &Shared, identifier: &str) -> Result<String, ErrorCode> {
    lookup(session, state, identifier).await.map(|(id, _)| id)
}

/// As [`resolve`], also returning whether a guest of this share created the Agent.
async fn lookup(
    session: &Session,
    state: &Shared,
    identifier: &str,
) -> Result<(String, bool), ErrorCode> {
    let execution = state
        .provider
        .agent_execution
        .as_ref()
        .ok_or(ErrorCode::Forbidden)?;
    let value = execution
        .execute("agent.get.request", json!({"agentId": identifier}))
        .await
        .map_err(|code| conceal(code.into()))?;
    let agent = &value["agent"];
    if agent["workspaceId"].as_str() != Some(session.workspace_id()) {
        return Err(ErrorCode::Forbidden);
    }
    let guest_created = created_under(agent, &session.registration.grant().share_id.to_string());
    agent["id"]
        .as_str()
        .map(|id| (id.to_owned(), guest_created))
        .ok_or(ErrorCode::Forbidden)
}

/// Whether `agent` (an `agent.get` payload) was created by a guest of `share`.
pub(super) fn created_under(agent: &Value, share: &str) -> bool {
    agent["labels"][GUEST_SHARE_LABEL].as_str() == Some(share)
}

/// Require the Agent named by `key` to be guest-created under this share and pin its full ID.
async fn driven(
    session: &Session,
    state: &Shared,
    params: &mut Value,
    key: &str,
) -> Result<(), ErrorCode> {
    let identifier = params[key].as_str().ok_or(ErrorCode::Forbidden)?;
    let (id, guest_created) = lookup(session, state, identifier).await?;
    if !guest_created {
        return Err(ErrorCode::Forbidden);
    }
    params[key] = json!(id);
    Ok(())
}

/// Require the Agent named by `key` to be in the workspace and pin its full ID.
async fn scoped(
    session: &Session,
    state: &Shared,
    params: &mut Value,
    key: &str,
) -> Result<(), ErrorCode> {
    let identifier = params[key].as_str().ok_or(ErrorCode::Forbidden)?;
    let id = resolve(session, state, identifier).await?;
    params[key] = json!(id);
    Ok(())
}

/// Require every Agent listed under `key` to be in the workspace, and guest-created when
/// `drive` is set, and pin their full IDs.
async fn many(
    session: &Session,
    state: &Shared,
    params: &mut Value,
    key: &str,
    drive: bool,
) -> Result<(), ErrorCode> {
    let Some(ids) = params.get(key).filter(|ids| !ids.is_null()) else {
        return Ok(());
    };
    let ids = ids.as_array().ok_or(ErrorCode::InvalidMessage)?;
    if ids.len() > MAX_IDS {
        return Err(ErrorCode::InvalidMessage);
    }
    let mut resolved = Vec::with_capacity(ids.len());
    for identifier in ids {
        let identifier = identifier.as_str().ok_or(ErrorCode::InvalidMessage)?;
        let (id, guest_created) = lookup(session, state, identifier).await?;
        if drive && !guest_created {
            return Err(ErrorCode::Forbidden);
        }
        resolved.push(id);
    }
    params[key] = json!(resolved);
    Ok(())
}

async fn terminals(session: &Session, state: &Shared, params: &Value) -> Result<(), ErrorCode> {
    let Some(ids) = params.get("terminalIds").filter(|ids| !ids.is_null()) else {
        return Ok(());
    };
    let ids = ids.as_array().ok_or(ErrorCode::InvalidMessage)?;
    if ids.len() > MAX_IDS {
        return Err(ErrorCode::InvalidMessage);
    }
    for id in ids {
        terminal(
            session,
            state,
            id.as_str().ok_or(ErrorCode::InvalidMessage)?,
        )
        .await?;
    }
    Ok(())
}

/// Force directory and history filters to the guest's workspace.
fn filter(session: &Session, params: &mut Value) -> Result<(), ErrorCode> {
    let request = object(params)?;
    let filter = request.entry("filter").or_insert_with(|| json!({}));
    object(filter)?.insert("workspaceIds".to_owned(), json!([session.workspace_id()]));
    Ok(())
}

/// Place a new Agent in the guest's workspace, attribute it, and refuse options that reach
/// beyond the workspace.
async fn create(session: &Session, state: &Shared, params: &mut Value) -> Result<(), ErrorCode> {
    let request = object(params)?;
    if CREATE_DENIED.iter().any(|key| present(request.get(*key))) {
        return Err(ErrorCode::Forbidden);
    }
    let config = request
        .get_mut("config")
        .and_then(Value::as_object_mut)
        .ok_or(ErrorCode::InvalidMessage)?;
    if CONFIG_DENIED.iter().any(|key| present(config.get(*key))) {
        return Err(ErrorCode::Forbidden);
    }
    config.insert("cwd".to_owned(), json!(session.root_text));
    let labels = object(request.entry("labels").or_insert_with(|| json!({})))?;
    if labels.keys().any(|key| key.starts_with(GUEST_LABEL_PREFIX)) {
        return Err(ErrorCode::Forbidden);
    }
    let grant = session.registration.grant();
    labels.insert(
        GUEST_SHARE_LABEL.to_owned(),
        json!(grant.share_id.to_string()),
    );
    labels.insert(
        GUEST_USER_LABEL.to_owned(),
        json!(grant.user_id.to_string()),
    );
    request.insert("workspaceId".to_owned(), json!(session.workspace_id()));
    if present(params.get("callerAgentId")) {
        scoped(session, state, params, "callerAgentId").await?;
    }
    Ok(())
}
