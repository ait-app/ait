//! Resource closure between metadata archive and owned checkout removal.

use filesystem::service::worktrees::{
    ArchiveScope, ArchiveWorktree, PendingArchive, WorktreesError,
};
use model::outbound::QueueError;
use model::{Context, ErrorCode};
use serde_json::{Value, json};

use crate::Shared;

pub(super) async fn request(
    context: &mut Option<Context<'_>>,
    state: &Shared,
) -> Result<(), QueueError> {
    let Some(mut context) = context.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "workspace.archive.request"
                | "project.remove.request"
                | "workspace.worktree.archive.request"
        )
    }) else {
        return Ok(());
    };
    let result = if context.request.method == "workspace.worktree.archive.request" {
        worktree(state, std::mem::take(&mut context.request.params)).await
    } else {
        metadata(state, &mut context).await
    };
    context.respond(result)
}

async fn metadata(state: &Shared, context: &mut Context<'_>) -> Result<Value, ErrorCode> {
    let (value, workspace_ids) = context
        .call(
            state.metadata.directory.clone(),
            ErrorCode::RegistryIo,
            prepare_metadata,
        )
        .await?;
    let mut plans = Vec::new();
    if state.filesystem.worktrees.is_some() {
        for workspace_id in &workspace_ids {
            let input = ArchiveWorktree {
                workspace_id: Some(workspace_id.clone()),
                scope: ArchiveScope::Workspace,
                worktree_path: None,
                repo_root: None,
                worktree_slug: None,
                branch_name: None,
            };
            plans.push(
                begin(state, input)
                    .await?
                    .map_err(|_| ErrorCode::RegistryIo)?,
            );
        }
    }
    retire(state, workspace_ids).await?;
    for plan in plans {
        finish(state, plan)
            .await?
            .map_err(|_| ErrorCode::RegistryIo)?;
    }
    Ok(value)
}

fn prepare_metadata(
    directory: &mut metadata::service::directory::Directory,
    method: &str,
    params: Value,
) -> Result<(Value, Vec<String>), ErrorCode> {
    let mut ids = if method == "project.remove.request" {
        let project = params["projectId"]
            .as_str()
            .ok_or(ErrorCode::InvalidMessage)?;
        // Include archived records so a retry can finish native cleanup after partial failure.
        directory
            .list_workspaces()
            .map_err(|_| ErrorCode::RegistryIo)?
            .into_iter()
            .filter(|workspace| workspace.project_id == project)
            .map(|workspace| workspace.workspace_id)
            .collect()
    } else {
        Vec::new()
    };
    let value = metadata::rpc::directory::execute(directory, method, params)?;
    if method == "workspace.archive.request"
        && value["archivedAt"].is_string()
        && let Some(id) = value["workspaceId"].as_str()
    {
        ids.push(id.to_owned());
    }
    Ok((value, ids))
}

async fn worktree(state: &Shared, params: Value) -> Result<Value, ErrorCode> {
    let request = serde_json::from_value(params).map_err(|_| ErrorCode::InvalidMessage)?;
    let plan = match begin(state, filesystem::rpc::worktrees::archive_input(request)).await? {
        Ok(plan) => plan,
        Err(error) => return Ok(failure(&error)),
    };
    let agents = retire(state, plan.workspace_ids.clone()).await?;
    match finish(state, plan).await? {
        Ok(()) => Ok(json!({"success":true,"removedAgents":agents,"error":null})),
        Err(error) => Ok(failure(&error)),
    }
}

async fn begin(
    state: &Shared,
    input: ArchiveWorktree,
) -> Result<Result<PendingArchive, WorktreesError>, ErrorCode> {
    state
        .runtime
        .run(
            state.filesystem.worktrees.clone(),
            ErrorCode::RegistryIo,
            move |worktrees| Ok(worktrees.begin_archive(&input, &chrono::Utc::now().to_rfc3339())),
        )
        .await
}

async fn finish(
    state: &Shared,
    plan: PendingArchive,
) -> Result<Result<(), WorktreesError>, ErrorCode> {
    state
        .runtime
        .run(
            state.filesystem.worktrees.clone(),
            ErrorCode::RegistryIo,
            move |worktrees| Ok(worktrees.finish_archive(plan).map(|_| ())),
        )
        .await
}

async fn retire(state: &Shared, workspace_ids: Vec<String>) -> Result<Vec<String>, ErrorCode> {
    if workspace_ids.is_empty() {
        return Ok(Vec::new());
    }
    let agents =
        provider::dispatch::retire_workspaces(&state.provider, workspace_ids.clone()).await?;
    terminal::dispatch::reconcile_workspaces(&state.terminal, workspace_ids.clone()).await?;
    if state.metadata.workspace_automation.is_some() {
        state
            .runtime
            .run(
                state.metadata.workspace_automation.clone(),
                ErrorCode::RegistryIo,
                move |automation| {
                    automation
                        .close_workspaces(&workspace_ids)
                        .map_err(|_| ErrorCode::RegistryIo)
                },
            )
            .await?;
    }
    Ok(agents)
}

fn failure(error: &WorktreesError) -> Value {
    json!({"success":false,"removedAgents":[],"error":filesystem::rpc::worktrees::checkout_error(error)})
}
