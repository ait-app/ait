use std::collections::BTreeMap;

use super::{AgentManager, AgentManagerError, now_timestamp};

#[derive(Debug, Default)]
pub(super) struct AutoArchives {
    states: BTreeMap<String, bool>,
    worktrees: BTreeMap<String, WorktreeCleanup>,
}

#[derive(Debug, Clone)]
struct WorktreeCleanup {
    workspace: String,
    provisioning: std::sync::Arc<dyn model::workspace::worktrees::WorktreeProvisioning>,
}

/// Completed turn retirement, executed by the scheduler after fencing related writers.
#[derive(Debug, Clone)]
pub(crate) struct Retirement {
    pub(crate) id: String,
    worktree: Option<WorktreeCleanup>,
}

impl Retirement {
    /// Return the managed Workspace that must retire before filesystem cleanup.
    pub(crate) fn workspace(&self) -> Option<&str> {
        self.worktree
            .as_ref()
            .map(|worktree| worktree.workspace.as_str())
    }

    /// Remove a managed worktree only after all affected native writers have closed.
    /// # Errors
    /// Returns a cleanup error without removing the pending retirement.
    pub(crate) fn cleanup(&self) -> Result<(), crate::rpc::ErrorCode> {
        if let Some(worktree) = &self.worktree {
            worktree
                .provisioning
                .archive(&worktree.workspace, &now_timestamp())
                .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        }
        Ok(())
    }
}

impl AutoArchives {
    pub(super) fn completed(&mut self, id: &str) {
        if let Some(ready) = self.states.get_mut(id) {
            *ready = true;
        }
    }
}

impl AgentManager {
    /// Archive the Agent and its newly provisioned managed worktree after its first terminal event.
    pub(crate) fn auto_archive_worktree_on_finish(
        &mut self,
        id: String,
        workspace: String,
        provisioning: std::sync::Arc<dyn model::workspace::worktrees::WorktreeProvisioning>,
    ) {
        self.auto_archive_on_finish(id.clone());
        self.auto_archives.worktrees.insert(
            id,
            WorktreeCleanup {
                workspace,
                provisioning,
            },
        );
    }

    /// Archive this Agent on its first completed, failed, or cancelled turn in this process.
    pub(crate) fn auto_archive_on_finish(&mut self, id: String) {
        self.auto_archives.states.insert(id, false);
    }

    pub(super) async fn archive_finished(&mut self) -> Result<(), AgentManagerError> {
        let inactive: Vec<_> = self
            .auto_archives
            .states
            .iter()
            .filter(|(_, ready)| !**ready)
            .map(|(id, _)| id.clone())
            .collect();
        for id in inactive {
            if self
                .registry
                .get(&id)
                .map_err(|_| AgentManagerError::Registry)?
                .is_none_or(|record| record.archived_at.is_some())
            {
                self.auto_archives.states.remove(&id);
                self.auto_archives.worktrees.remove(&id);
            }
        }
        let ready: Vec<_> = self
            .auto_archives
            .states
            .iter()
            .filter(|(_, ready)| **ready)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ready {
            if let Some(owner) = &self.owner {
                owner
                    .retire(Retirement {
                        id: id.clone(),
                        worktree: self.auto_archives.worktrees.get(&id).cloned(),
                    })
                    .map_err(|_| AgentManagerError::Registry)?;
                self.auto_archives.states.remove(&id);
                self.auto_archives.worktrees.remove(&id);
                continue;
            }
            if let Some(worktree) = self.auto_archives.worktrees.get(&id) {
                let workspace = worktree.workspace.clone();
                self.archive_workspace_agents(std::slice::from_ref(&workspace))?;
            }
            let result = super::super::agent_runtime::archive::archive(
                self.registry.as_ref(),
                &id,
                &now_timestamp(),
            );
            if result.is_err()
                && !matches!(
                    result,
                    Err(super::super::agent_runtime::AgentRuntimeError::NotFound(_))
                )
            {
                return Err(AgentManagerError::Registry);
            }
            if let Some(timeline) = &self.timeline {
                timeline
                    .cancel_inputs(&id)
                    .map_err(|_| AgentManagerError::Registry)?;
            }
            self.reconcile().await?;
            if let Some(worktree) = self.auto_archives.worktrees.get(&id) {
                let provisioning = worktree.provisioning.clone();
                let workspace = worktree.workspace.clone();
                tokio::task::spawn_blocking(move || {
                    provisioning.archive(&workspace, &now_timestamp())
                })
                .await
                .map_err(|_| AgentManagerError::Registry)?
                .map_err(|_| AgentManagerError::Registry)?;
            }
            self.auto_archives.states.remove(&id);
            self.auto_archives.worktrees.remove(&id);
        }
        Ok(())
    }

    pub(crate) fn archive_workspace_agents(
        &self,
        workspaces: &[String],
    ) -> Result<Vec<String>, AgentManagerError> {
        let timestamp = now_timestamp();
        let ids = super::super::agent_runtime::archive::archive_workspaces(
            self.registry.as_ref(),
            workspaces,
            &timestamp,
        )
        .map_err(|_| AgentManagerError::Registry)?;
        for id in &ids {
            if let Some(timeline) = &self.timeline {
                timeline
                    .cancel_inputs(id)
                    .map_err(|_| AgentManagerError::Registry)?;
            }
        }
        Ok(ids)
    }
}
