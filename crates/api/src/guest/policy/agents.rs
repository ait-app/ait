//! Agent methods: only Agents of workspaces in the shared scope, never the daemon MCP.

use provider::service::agent_mcp::{GUEST_SHARE_LABEL, GUEST_USER_LABEL};
use serde_json::{Value, json};

use super::super::Reach;
use super::{ErrorCode, Rule, Session, conceal, object, present, reached, terminals};
use crate::Shared;

/// Agent creation options that could place an Agent outside an existing workspace.
const PLACEMENT_DENIED: [&str; 3] = ["worktree", "worktreeName", "git"];

/// Agent creation options no guest may choose: a custom environment reaches beyond the
/// Agent's tools.
const CREATE_DENIED: [&str; 1] = ["env"];

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
        Rule::AgentFilter => filter(session, state, params).await,
        Rule::AgentSubscription => many(session, state, params, "agentIds", false).await,
        Rule::ItemsClose => {
            many(session, state, params, "agentIds", true).await?;
            terminals(session, params)
        }
        Rule::Open
        | Rule::Catalog
        | Rule::Workspace
        | Rule::WorkspaceManage
        | Rule::WorkspaceList
        | Rule::WorkspaceCreate
        | Rule::WorktreeCreate
        | Rule::WorktreeArchive
        | Rule::WorkspaceOpen
        | Rule::ProjectList
        | Rule::Project
        | Rule::Paths(_)
        | Rule::Node
        | Rule::TerminalCreate
        | Rule::TerminalFilter
        | Rule::Terminal
        | Rule::TerminalDrive
        | Rule::Path
        | Rule::Download
        | Rule::Deny => Err(ErrorCode::Forbidden),
    }
}

/// Resolve `identifier` like the provider (full ID, unique prefix, exact title) and return
/// the full ID when the Agent belongs to a workspace in scope.
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
    let workspace = agent["workspaceId"].as_str().ok_or(ErrorCode::Forbidden)?;
    reached(session, state, workspace).await?;
    let grant = session.registration.grant();
    let guest_created = created_under(
        agent,
        &grant.share_id.to_string(),
        &grant.user_id.to_string(),
    );
    agent["id"]
        .as_str()
        .map(|id| (id.to_owned(), guest_created))
        .ok_or(ErrorCode::Forbidden)
}

/// Whether `agent` (an `agent.get` payload) was created by visitor `user` through `share`.
/// Other visitors of the same share only read it: instructions, cancellation and
/// permission decisions stay with its creator, and revoking a visitor ends what they ran.
pub(super) fn created_under(agent: &Value, share: &str, user: &str) -> bool {
    agent["labels"][GUEST_SHARE_LABEL].as_str() == Some(share)
        && agent["labels"][GUEST_USER_LABEL].as_str() == Some(user)
}

/// Require the Agent named by `key` to be created by this visitor under this share and pin
/// its full ID.
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

/// Require the Agent named by `key` to be in scope and pin its full ID.
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

/// Require every Agent listed under `key` to be in scope, and guest-created when
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

fn terminals(session: &Session, params: &Value) -> Result<(), ErrorCode> {
    let Some(ids) = params.get("terminalIds").filter(|ids| !ids.is_null()) else {
        return Ok(());
    };
    let ids = ids.as_array().ok_or(ErrorCode::InvalidMessage)?;
    if ids.len() > MAX_IDS {
        return Err(ErrorCode::InvalidMessage);
    }
    // Closing kills the terminal, so only the visitor's own terminals may be closed.
    for id in ids {
        terminals::owned(session, id.as_str().ok_or(ErrorCode::InvalidMessage)?)?;
    }
    Ok(())
}

/// Force directory and history filters to the workspaces in scope: the shared workspace,
/// or the requested workspaces of the shared project (all of them when none is requested).
/// A node share keeps the filter.
async fn filter(session: &Session, state: &Shared, params: &mut Value) -> Result<(), ErrorCode> {
    let workspaces = match &session.reach {
        Reach::Node => return Ok(()),
        Reach::Workspace { id, .. } => vec![id.clone()],
        Reach::Project { .. } => {
            let mut workspaces = session
                .project_workspace_ids(state)
                .await
                .map_err(conceal)?;
            if let Some(requested) = params["filter"]["workspaceIds"].as_array() {
                workspaces.retain(|id| requested.iter().any(|requested| requested == id));
            }
            workspaces
        }
    };
    let request = object(params)?;
    let filter = request.entry("filter").or_insert_with(|| json!({}));
    object(filter)?.insert("workspaceIds".to_owned(), json!(workspaces));
    Ok(())
}

/// Place a new Agent in a workspace in scope, attribute it, and refuse options that reach
/// beyond the scope. A workspace share forces its workspace; a project share requires a
/// workspace of the project; a node share places the Agent where the guest asks, worktree
/// placement included.
async fn create(session: &Session, state: &Shared, params: &mut Value) -> Result<(), ErrorCode> {
    let placement = placement(session, state, params).await?;
    let request = object(params)?;
    let given = |keys: &[&str]| keys.iter().any(|key| present(request.get(*key)));
    if given(&CREATE_DENIED) || (placement.is_some() && given(&PLACEMENT_DENIED)) {
        return Err(ErrorCode::Forbidden);
    }
    let config = request
        .get_mut("config")
        .and_then(Value::as_object_mut)
        .ok_or(ErrorCode::InvalidMessage)?;
    if CONFIG_DENIED.iter().any(|key| present(config.get(*key))) {
        return Err(ErrorCode::Forbidden);
    }
    if let Some((_, directory)) = &placement {
        config.insert("cwd".to_owned(), json!(directory));
    }
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
    if let Some((workspace, _)) = placement {
        request.insert("workspaceId".to_owned(), json!(workspace));
    }
    if present(params.get("callerAgentId")) {
        scoped(session, state, params, "callerAgentId").await?;
    }
    Ok(())
}

/// The workspace and directory a new Agent is forced into; `None` for a node share.
async fn placement(
    session: &Session,
    state: &Shared,
    params: &Value,
) -> Result<Option<(String, String)>, ErrorCode> {
    let workspace = match &session.reach {
        Reach::Node => return Ok(None),
        Reach::Workspace { id, .. } => id.clone(),
        Reach::Project { .. } => params["workspaceId"]
            .as_str()
            .ok_or(ErrorCode::Forbidden)?
            .to_owned(),
    };
    let directory = session
        .directory(state, &workspace)
        .await
        .map_err(conceal)?
        .ok_or(ErrorCode::Forbidden)?;
    Ok(Some((workspace, directory)))
}
