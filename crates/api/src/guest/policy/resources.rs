//! Protect owner-managed configuration and bind remote reads to the shared checkout.

use std::path::{Path, PathBuf};

use domain::workspace::provisioning::{LEGACY_PROJECT_CONFIG_FILE_NAME, PROJECT_CONFIG_FILE_NAME};
use model::ErrorCode;
use model::share::ResourceKind;
use serde_json::Value;

use super::{Session, inside};
use crate::Shared;
use crate::guest::Reach;
use crate::shares::rpc::{active_project, active_workspace};

/// Check additional owner-only effects of otherwise scoped requests.
/// # Errors
/// Returns `Forbidden` for owner-only effects or unverified paths/repositories, and service errors.
pub(super) async fn check(
    session: &Session,
    state: &Shared,
    method: &str,
    params: &Value,
) -> Result<(), ErrorCode> {
    match method {
        "workspace.archive.request" | "workspace.worktree.archive.request" => {
            archive(session, state, method, params).await
        }
        "fs.file.write.request"
        | "fs.entry.create.request"
        | "fs.entry.rename.request"
        | "fs.entry.duplicate.request"
        | "fs.entry.delete.request" => configuration(session, state, method, params).await,
        "checkout.pr.timeline.request"
        | "checkout.forge.get_check_details.request"
        | "checkout.github.get_check_details.request" => repository(session, state, params).await,
        _ => Ok(()),
    }
}

async fn archive(
    session: &Session,
    state: &Shared,
    method: &str,
    params: &Value,
) -> Result<(), ErrorCode> {
    if session.project_id().is_none() {
        return Ok(());
    }
    let workspace = params["workspaceId"].as_str();
    if method == "workspace.archive.request"
        || (params["scope"] == "workspace" && workspace.is_some())
    {
        if workspace.is_some_and(|id| state.shares.has_settings(ResourceKind::Workspace, id)) {
            return Err(ErrorCode::Forbidden);
        }
        return Ok(());
    }
    // A worktree archive can retire multiple registrations, even from other projects.
    let workspaces = state
        .runtime
        .run_queued(
            state.metadata.directory.clone(),
            ErrorCode::RegistryIo,
            |directory| {
                directory
                    .list_workspaces()
                    .map_err(|_| ErrorCode::RegistryIo)
            },
        )
        .await?;
    let explicit = params["worktreePath"]
        .as_str()
        .filter(|path| !path.trim().is_empty());
    if explicit.is_none()
        && params["branchName"]
            .as_str()
            .is_some_and(|name| !name.trim().is_empty())
    {
        return Err(ErrorCode::Forbidden);
    }
    let path = explicit
        .or_else(|| {
            workspace
                .and_then(|id| workspaces.iter().find(|item| item.workspace_id == id))
                .map(|item| item.cwd.as_str())
        })
        .ok_or(ErrorCode::Forbidden)?;
    if path != path.trim() {
        return Err(ErrorCode::Forbidden);
    }
    let target = resolved(Path::new(path)).await?;
    for item in workspaces {
        if !state
            .shares
            .has_settings(ResourceKind::Workspace, &item.workspace_id)
        {
            continue;
        }
        let root = item.worktree_root.as_deref().unwrap_or(&item.cwd);
        if resolved(Path::new(&item.cwd)).await?.starts_with(&target)
            || resolved(Path::new(root)).await? == target
        {
            return Err(ErrorCode::Forbidden);
        }
    }
    Ok(())
}

async fn configuration(
    session: &Session,
    state: &Shared,
    method: &str,
    params: &Value,
) -> Result<(), ErrorCode> {
    let cwd = params["cwd"].as_str().ok_or(ErrorCode::Forbidden)?;
    inside(session, state, cwd).await?;
    let project = match &session.reach {
        Reach::Project { id, .. } => id.clone(),
        Reach::Workspace { id, .. } => {
            active_workspace(state, id)
                .await?
                .ok_or(ErrorCode::Forbidden)?
                .project_id
        }
        Reach::Node => return Ok(()),
    };
    let project = active_project(state, &project)
        .await?
        .ok_or(ErrorCode::Forbidden)?;
    // The config store falls back to the legacy name, so both files steer setup commands.
    let root = Path::new(&project.root_path);
    let protected = [
        resolved(&root.join(PROJECT_CONFIG_FILE_NAME)).await?,
        resolved(&root.join(LEGACY_PROJECT_CONFIG_FILE_NAME)).await?,
    ];
    let path = if method == "fs.entry.create.request" {
        Path::new(cwd)
            .join(params["parentPath"].as_str().ok_or(ErrorCode::Forbidden)?)
            .join(params["name"].as_str().ok_or(ErrorCode::Forbidden)?)
    } else {
        Path::new(cwd).join(params["path"].as_str().ok_or(ErrorCode::Forbidden)?)
    };
    let replaces = |target: PathBuf| protected.iter().any(|file| file.starts_with(&target));
    if replaces(resolved(&path).await?) {
        return Err(ErrorCode::Forbidden);
    }
    if method == "fs.entry.rename.request" {
        let renamed = path.with_file_name(params["name"].as_str().ok_or(ErrorCode::Forbidden)?);
        if replaces(resolved(&renamed).await?) {
            return Err(ErrorCode::Forbidden);
        }
    }
    Ok(())
}

/// Resolve existing files or a new leaf beneath an existing directory, following aliases.
async fn resolved(path: &Path) -> Result<PathBuf, ErrorCode> {
    match tokio::fs::canonicalize(path).await {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or(ErrorCode::Forbidden)?;
            let name = path.file_name().ok_or(ErrorCode::Forbidden)?;
            tokio::fs::canonicalize(parent)
                .await
                .map(|parent| parent.join(name))
                .map_err(|_| ErrorCode::Forbidden)
        }
        Err(_) => Err(ErrorCode::Forbidden),
    }
}

async fn repository(session: &Session, state: &Shared, params: &Value) -> Result<(), ErrorCode> {
    let cwd = params["cwd"].as_str().ok_or(ErrorCode::Forbidden)?;
    inside(session, state, cwd).await?;
    let owner = params["repoOwner"].as_str().map(str::to_owned);
    let name = params["repoName"].as_str().map(str::to_owned);
    // GitLab uses the checkout's project identity directly; no override is required.
    if owner.is_none() && name.is_none() {
        return Ok(());
    }
    let (Some(owner), Some(name)) = (owner, name) else {
        return Err(ErrorCode::Forbidden);
    };
    let cwd = cwd.to_owned();
    let admitted = state
        .run(
            state.filesystem.forge.clone(),
            ErrorCode::Forbidden,
            move |forge| {
                forge
                    .admits_repository(&cwd, &owner, &name)
                    .map_err(|_| ErrorCode::Forbidden)
            },
        )
        .await?;
    if admitted {
        Ok(())
    } else {
        Err(ErrorCode::Forbidden)
    }
}
