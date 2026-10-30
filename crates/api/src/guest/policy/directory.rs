//! Workspace and project lifecycle methods, available to project and node shares.
//!
//! A project share places every new workspace or worktree in its project and reaches only
//! the project's directories; a node share keeps the request. Neither may start an initial
//! Agent through workspace creation, because that Agent would bypass the guest Agent rules.

use serde_json::{Value, json};

use super::{ErrorCode, Rule, Session, conceal, inside, object, present, reached};
use crate::Shared;

pub(super) async fn confine(
    session: &Session,
    state: &Shared,
    rule: Rule,
    params: &mut Value,
) -> Result<(), ErrorCode> {
    if rule == Rule::WorkspaceCreate && present(params.get("agent")) {
        return Err(ErrorCode::Forbidden);
    }
    let Some(project) = session.project_id() else {
        // Node shares reach every workspace, project and path.
        return Ok(());
    };
    match rule {
        Rule::WorkspaceCreate => workspace_create(session, state, project, params).await,
        Rule::WorktreeCreate => {
            let cwd = params["cwd"].as_str().ok_or(ErrorCode::Forbidden)?;
            inside(session, state, cwd).await?;
            object(params)?.insert("projectId".to_owned(), json!(project));
            Ok(())
        }
        Rule::WorktreeArchive => worktree_archive(session, state, params).await,
        Rule::WorkspaceOpen => {
            let cwd = params["cwd"].as_str().ok_or(ErrorCode::Forbidden)?;
            if session.opens(state, cwd).await.map_err(conceal)? {
                Ok(())
            } else {
                Err(ErrorCode::Forbidden)
            }
        }
        Rule::Project => {
            if params["projectId"].as_str() == Some(project) {
                Ok(())
            } else {
                Err(ErrorCode::Forbidden)
            }
        }
        Rule::Paths(keys) => paths(session, state, params, keys).await,
        Rule::Open
        | Rule::Catalog
        | Rule::Workspace
        | Rule::WorkspaceManage
        | Rule::WorkspaceList
        | Rule::ProjectList
        | Rule::Node
        | Rule::AgentCreate
        | Rule::Agent(_)
        | Rule::AgentDrive(_)
        | Rule::AgentUpdate
        | Rule::AgentCommands
        | Rule::AgentFilter
        | Rule::AgentSubscription
        | Rule::ItemsClose
        | Rule::TerminalCreate
        | Rule::TerminalFilter
        | Rule::Terminal
        | Rule::Path
        | Rule::Download
        | Rule::Deny => Err(ErrorCode::Forbidden),
    }
}

/// Require the source directory of a new workspace in scope and own it by `project`.
async fn workspace_create(
    session: &Session,
    state: &Shared,
    project: &str,
    params: &mut Value,
) -> Result<(), ErrorCode> {
    let source = &params["source"];
    let key = match source["kind"].as_str() {
        Some("directory") => "path",
        Some("worktree") => "cwd",
        _ => return Err(ErrorCode::InvalidMessage),
    };
    // A worktree source without `cwd` starts from the project root.
    if let Some(path) = source.get(key).filter(|path| !path.is_null()) {
        inside(session, state, path.as_str().ok_or(ErrorCode::Forbidden)?).await?;
    } else if key == "path" {
        return Err(ErrorCode::InvalidMessage);
    }
    object(&mut params["source"])?.insert("projectId".to_owned(), json!(project));
    Ok(())
}

/// Require every named workspace and path of a worktree archive to be in scope.
async fn worktree_archive(
    session: &Session,
    state: &Shared,
    params: &Value,
) -> Result<(), ErrorCode> {
    let workspace = params.get("workspaceId").filter(|id| !id.is_null());
    if let Some(id) = workspace {
        reached(session, state, id.as_str().ok_or(ErrorCode::Forbidden)?).await?;
    }
    let named = contained(session, state, params, &["worktreePath", "repoRoot"]).await?;
    if named || workspace.is_some() {
        Ok(())
    } else {
        Err(ErrorCode::Forbidden)
    }
}

/// Require each of `keys` that is present to name a path in scope, and one to be present.
async fn paths(
    session: &Session,
    state: &Shared,
    params: &Value,
    keys: &[&str],
) -> Result<(), ErrorCode> {
    if contained(session, state, params, keys).await? {
        Ok(())
    } else {
        Err(ErrorCode::Forbidden)
    }
}

/// Require each of `keys` that is present to name a path in scope; returns whether any is.
async fn contained(
    session: &Session,
    state: &Shared,
    params: &Value,
    keys: &[&str],
) -> Result<bool, ErrorCode> {
    let mut found = false;
    for key in keys {
        let Some(path) = params.get(*key).filter(|path| !path.is_null()) else {
            continue;
        };
        inside(session, state, path.as_str().ok_or(ErrorCode::Forbidden)?).await?;
        found = true;
    }
    Ok(found)
}
