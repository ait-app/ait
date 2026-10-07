use std::path::Path;

use model::workspace::lifecycle::WorkspaceCreation;

use super::{CreateRequest, ErrorCode, ExecutionState};

impl ExecutionState {
    pub(super) async fn creation_placement(
        &self,
        request: &mut CreateRequest,
    ) -> Result<(String, bool), ErrorCode> {
        let caller = request
            .caller_agent_id
            .as_deref()
            .map(|id| {
                self.registry
                    .get(id)
                    .map_err(|_| ErrorCode::RegistryIo)?
                    .filter(|caller| caller.archived_at.is_none())
                    .ok_or(ErrorCode::AgentNotFound)
            })
            .transpose()?;
        if let Some(caller) = &caller {
            if request.labels.len() == 100 && !request.labels.contains_key("paseo.parent-agent-id")
            {
                return Err(ErrorCode::InvalidMessage);
            }
            request
                .labels
                .insert("paseo.parent-agent-id".to_owned(), caller.id.clone());
        }
        if let Some(worktree) = super::worktrees::intent(request)? {
            let id = self.create_worktree_placement(request, worktree).await?;
            return Ok((id, true));
        }
        let explicit = request.workspace_id.as_deref().filter(|id| !id.is_empty());
        let inherited = caller
            .as_ref()
            .filter(|_| explicit.is_none())
            .map(|caller| {
                caller
                    .workspace_id
                    .as_deref()
                    .filter(|id| !id.is_empty())
                    .ok_or(ErrorCode::InvalidMessage)
            })
            .transpose()?;
        let workspace_id = if let Some(id) = explicit.or(inherited) {
            let workspace = self
                .workspaces
                .get(id)
                .map_err(|_| ErrorCode::RegistryIo)?
                .ok_or(ErrorCode::InvalidMessage)?;
            // Explicit Workspace placement replaces a stale client cwd. Caller inheritance
            // preserves the caller's native cwd, independently of its owning Workspace cwd.
            let cwd = if explicit.is_some() {
                &workspace.cwd
            } else {
                &caller.as_ref().ok_or(ErrorCode::AgentNotFound)?.cwd
            };
            let id = self.workspace(Some(id), &workspace.cwd)?;
            request.config.cwd = directory(cwd)?;
            self.prepare_directory_git(request).await?;
            id
        } else {
            request.config.cwd = directory(&request.config.cwd)?;
            self.prepare_directory_git(request).await?;
            if let Some(directory) = &self.import_directory {
                directory
                    .create_workspace(WorkspaceCreation {
                        path: &request.config.cwd,
                        title: None,
                        project_id: None,
                        workspace_id: None,
                        expects_initial_agent: true,
                        timestamp: &chrono::Utc::now().to_rfc3339(),
                    })
                    .map_err(|_| ErrorCode::RegistryIo)?
                    .workspace_id
            } else {
                self.workspace(None, &request.config.cwd)?
            }
        };
        Ok((workspace_id, false))
    }
}

pub(super) fn directory(cwd: &str) -> Result<String, ErrorCode> {
    let cwd = Path::new(cwd)
        .canonicalize()
        .map_err(|_| ErrorCode::InvalidMessage)?;
    if !cwd.is_dir() {
        return Err(ErrorCode::InvalidMessage);
    }
    cwd.into_os_string()
        .into_string()
        .map_err(|_| ErrorCode::InvalidMessage)
}
