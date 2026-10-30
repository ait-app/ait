//! Composition adapter shared by explicit RPC and Provider-triggered worktree archives.

use std::sync::{Arc, Mutex};

use domain::workspace::worktrees::WorktreeError;
use metadata::worktrees::ports::WorktreeArchiveCleanup;

use crate::{ConfigError, Shared};

#[derive(Debug)]
struct ResourceCleanup {
    shares: Arc<crate::shares::Shares>,
    terminals: Option<Arc<Mutex<terminal::service::Terminals>>>,
    automation: Option<Arc<Mutex<metadata::automation::service::WorkspaceAutomation>>>,
}

impl WorktreeArchiveCleanup for ResourceCleanup {
    fn close_workspaces(&self, workspace_ids: &[String]) -> Result<(), WorktreeError> {
        self.shares.forget_workspaces(workspace_ids);
        if let Some(terminals) = &self.terminals {
            terminals
                .lock()
                .map_err(|_| WorktreeError::Io("Terminal cleanup unavailable".to_owned()))?
                .close_workspaces(workspace_ids)
                .map_err(|_| {
                    WorktreeError::Io("Terminal cleanup failed; retry archive".to_owned())
                })?;
        }
        if let Some(automation) = &self.automation {
            automation
                .lock()
                .map_err(|_| WorktreeError::Io("Workspace automation unavailable".to_owned()))?
                .close_workspaces(workspace_ids)
                .map_err(|_| {
                    WorktreeError::Io(
                        "Workspace automation cleanup failed; retry archive".to_owned(),
                    )
                })?;
        }
        Ok(())
    }
}

pub(super) fn configure(state: &Shared) -> Result<(), ConfigError> {
    if let Some(worktrees) = &state.metadata.worktrees {
        worktrees
            .lock()
            .map_err(|_| ConfigError::ServiceInitialization)?
            .set_archive_cleanup(Box::new(ResourceCleanup {
                shares: state.shares.clone(),
                terminals: state.terminal.terminals.clone(),
                automation: state.metadata.workspace_automation.clone(),
            }));
    }
    Ok(())
}
