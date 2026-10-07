//! Initial Agent creation shares a Workspace receipt and preserves source-relative placement.

use std::path::Path;

use model::creation::protocol::Snapshot;
use model::workspace::protocol::directory::WorkspaceCreateSource;
use serde::Deserialize;
use serde_json::Value;

use super::{CreateRequest, ErrorCode, ExecutionState, parse_creation};

/// Validate an embedded Agent before Workspace side effects and return its secret-free intent.
/// # Errors
/// Rejects invalid Agent fields, nested placement/creation controls or unsupported providers.
pub fn validate(params: &Value) -> Result<Value, ErrorCode> {
    if [
        "workspaceId",
        "worktree",
        "worktreeName",
        "git",
        "idempotencyKey",
        "subscribe",
    ]
    .iter()
    .any(|field| params.get(field).is_some())
    {
        return Err(ErrorCode::InvalidMessage);
    }
    let (request, intent) = parse_creation(params.clone())?;
    if let Some(id) = request.agent_id {
        uuid::Uuid::parse_str(&id).map_err(|_| ErrorCode::InvalidMessage)?;
    }
    Ok(intent)
}

#[derive(Deserialize)]
struct Input {
    agent: Value,
    creation: Snapshot,
    source: WorkspaceCreateSource,
}

impl ExecutionState {
    pub(super) async fn create_workspace_agent(
        &mut self,
        params: Value,
    ) -> Result<Value, ErrorCode> {
        let input: Input = super::decode(params)?;
        let request = self.workspace_agent_request(&input);
        let result = match request {
            Ok((request, workspace)) => {
                self.register_creation(request, &input.creation, workspace, false)
                    .await
            }
            Err(error) => Err(error),
        };
        if let Err(error) = &result {
            let creations = self.manager.creations();
            if let Some(mut latest) =
                creations.snapshot(input.creation.kind, &input.creation.idempotency_key)?
            {
                if latest.phase != "failed" && latest.phase != "completed" {
                    latest = creations.advance(&latest, "failed", None, Some(error.to_string()))?;
                }
                if let Some(id) = latest.agent_id.as_deref()
                    && latest.agent.is_none()
                    && self
                        .registry
                        .get(id)
                        .map_err(|_| ErrorCode::AgentIo)?
                        .is_none()
                    && self.manager.close(id).await.is_ok()
                {
                    creations.allow_initial_agent_retry(&latest)?;
                }
            }
        }
        result
    }

    fn workspace_agent_request(&self, input: &Input) -> Result<(CreateRequest, String), ErrorCode> {
        let (mut request, _) = parse_creation(input.agent.clone())?;
        let id = input
            .creation
            .workspace_id
            .as_ref()
            .ok_or(ErrorCode::InvalidMessage)?;
        let workspace = self
            .workspaces
            .get(id)
            .map_err(|_| ErrorCode::RegistryIo)?
            .ok_or(ErrorCode::InvalidMessage)?;
        self.workspace(Some(id), &workspace.cwd)?;
        let source = match &input.source {
            WorkspaceCreateSource::Directory { path, .. } => path.clone(),
            WorkspaceCreateSource::Worktree(source) => match &source.cwd {
                Some(cwd) => cwd.clone(),
                None => {
                    self.projects
                        .get(
                            source
                                .project_id
                                .as_deref()
                                .ok_or(ErrorCode::InvalidMessage)?,
                        )
                        .map_err(|_| ErrorCode::RegistryIo)?
                        .ok_or(ErrorCode::InvalidMessage)?
                        .root_path
                }
            },
        };
        let source = Path::new(&source)
            .canonicalize()
            .map_err(|_| ErrorCode::InvalidMessage)?;
        let draft = Path::new(&request.config.cwd)
            .canonicalize()
            .map_err(|_| ErrorCode::InvalidMessage)?;
        let relative = draft
            .strip_prefix(&source)
            .map_err(|_| ErrorCode::InvalidMessage)?;
        let target = Path::new(&workspace.cwd).join(relative);
        request.config.cwd =
            super::placement::directory(target.to_str().ok_or(ErrorCode::InvalidMessage)?)?;
        request.workspace_id = Some(id.clone());
        if let Some(parent) = &request.caller_agent_id {
            if request.labels.len() == 100 && !request.labels.contains_key("paseo.parent-agent-id")
            {
                return Err(ErrorCode::InvalidMessage);
            }
            let caller = self
                .registry
                .get(parent)
                .map_err(|_| ErrorCode::AgentIo)?
                .filter(|caller| caller.archived_at.is_none())
                .ok_or(ErrorCode::AgentNotFound)?;
            request
                .labels
                .insert("paseo.parent-agent-id".into(), caller.id);
        }
        Ok((request, id.clone()))
    }
}

#[cfg(test)]
mod tests;
