use model::workspace::worktrees::{DirectoryGit, WorktreeAction, WorktreeCreation};

use crate::protocol::creation::{GitAction, GitOptions, WorktreeTarget};

use super::{CreateRequest, ErrorCode, ExecutionState};

pub(super) fn intent(request: &CreateRequest) -> Result<Option<WorktreeCreation>, ErrorCode> {
    if request.worktree.is_some() && request.git.is_some() {
        return Err(ErrorCode::InvalidMessage);
    }
    let mut input = WorktreeCreation {
        cwd: Some(request.config.cwd.clone()),
        project_id: None,
        workspace_id: None,
        title: None,
        worktree_slug: None,
        ref_name: None,
        base_branch: None,
        branch_name: None,
        action: WorktreeAction::BranchOff,
        checkout_source: None,
        first_agent_prompt: request.initial_prompt.clone(),
        expects_initial_agent: true,
    };
    match &request.worktree {
        Some(WorktreeTarget::BranchOff { new_branch, base }) => {
            required(new_branch)?;
            if !new_branch.chars().any(|ch| ch.is_ascii_alphanumeric()) {
                return Err(ErrorCode::InvalidMessage);
            }
            if let Some(base) = base {
                required(base)?;
            }
            input.worktree_slug = Some(new_branch.clone());
            input.ref_name.clone_from(base);
        }
        Some(WorktreeTarget::CheckoutBranch { branch }) => {
            required(branch)?;
            input.action = WorktreeAction::Checkout;
            input.ref_name = Some(branch.clone());
        }
        Some(WorktreeTarget::CheckoutPr { pr_number }) => {
            if *pr_number == 0 || *pr_number > 9_007_199_254_740_991 {
                return Err(ErrorCode::InvalidMessage);
            }
            input.action = WorktreeAction::Checkout;
            input.checkout_source = Some(model::workspace::worktrees::WorktreeChangeRequest {
                forge: Some("github".into()),
                number: *pr_number,
                project_path: None,
            });
        }
        None => {
            let fallback = request
                .worktree_name
                .as_ref()
                .filter(|name| !name.is_empty())
                .map(|name| GitOptions {
                    create_worktree: true,
                    create_new_branch: true,
                    new_branch_name: Some(name.clone()),
                    worktree_slug: Some(name.clone()),
                    ..GitOptions::default()
                });
            let Some(git) = request.git.as_ref().or(fallback.as_ref()) else {
                return Ok(None);
            };
            if !legacy(&mut input, git)? {
                return Ok(None);
            }
        }
    }
    Ok(Some(input))
}

fn legacy(input: &mut WorktreeCreation, git: &GitOptions) -> Result<bool, ErrorCode> {
    if git
        .github_pr_number
        .is_some_and(|number| number == 0 || number > 9_007_199_254_740_991)
    {
        return Err(ErrorCode::InvalidMessage);
    }
    if git.create_new_branch
        && git
            .new_branch_name
            .as_deref()
            .is_none_or(|name| name.trim().is_empty())
    {
        return Err(ErrorCode::InvalidMessage);
    }
    if git
        .new_branch_name
        .as_ref()
        .filter(|_| git.create_new_branch)
        .is_some_and(|name| !name.chars().any(|ch| ch.is_ascii_alphanumeric()))
        || git
            .worktree_slug
            .as_ref()
            .is_some_and(|name| !name.chars().any(|ch| ch.is_ascii_alphanumeric()))
    {
        return Err(ErrorCode::InvalidMessage);
    }
    if !git.create_worktree {
        return Ok(false);
    }
    input.checkout_source = git
        .checkout_source
        .clone()
        .map(model::workspace::protocol::worktree_source::ChangeRequestCheckoutSource::into_intent)
        .or_else(|| {
            git.github_pr_number.map(
                |number| model::workspace::worktrees::WorktreeChangeRequest {
                    forge: Some("github".into()),
                    number,
                    project_path: None,
                },
            )
        });
    input.worktree_slug = git
        .worktree_slug
        .clone()
        .or_else(|| git.new_branch_name.clone());
    input.ref_name.clone_from(&git.ref_name);
    input.base_branch.clone_from(&git.base_branch);
    input.action = match git.action.unwrap_or(GitAction::BranchOff) {
        GitAction::BranchOff => WorktreeAction::BranchOff,
        GitAction::Checkout => WorktreeAction::Checkout,
    };
    Ok(true)
}

fn required(value: &str) -> Result<(), ErrorCode> {
    if value.trim().is_empty() || value.len() > 1024 || value.chars().any(char::is_control) {
        return Err(ErrorCode::InvalidMessage);
    }
    Ok(())
}

impl ExecutionState {
    pub(super) async fn prepare_directory_git(
        &self,
        request: &CreateRequest,
    ) -> Result<(), ErrorCode> {
        let Some(git) = &request.git else {
            return Ok(());
        };
        let base = git
            .base_branch
            .as_deref()
            .map(str::trim)
            .filter(|base| !base.is_empty())
            .map(str::to_owned);
        let intent = if git.create_new_branch {
            DirectoryGit::BranchOff {
                branch: git
                    .new_branch_name
                    .clone()
                    .ok_or(ErrorCode::InvalidMessage)?,
                base,
            }
        } else if let Some(branch) = base {
            DirectoryGit::Checkout { branch }
        } else {
            return Ok(());
        };
        let provisioning = self
            .import_directory
            .as_ref()
            .and_then(|directory| directory.shared_worktrees())
            .ok_or(ErrorCode::UnsupportedCapability)?;
        let cwd = request.config.cwd.clone();
        tokio::task::spawn_blocking(move || provisioning.prepare_directory(&cwd, &intent))
            .await
            .map_err(|_| ErrorCode::RegistryIo)?
            .map_err(|_| ErrorCode::InvalidMessage)
    }

    pub(super) async fn start_worktree_setup(&self, workspace: &str) {
        if let Some(automation) = self.workspace_automation.clone() {
            let workspace = workspace.to_owned();
            // Setup has its own observable failure status; it does not undo Agent creation.
            let _ = tokio::task::spawn_blocking(move || automation.start_created_setup(&workspace))
                .await;
        }
    }

    pub(super) async fn create_worktree_placement(
        &self,
        request: &mut CreateRequest,
        mut input: WorktreeCreation,
    ) -> Result<String, ErrorCode> {
        input.cwd = Some(super::placement::directory(&request.config.cwd)?);
        let provisioning = self
            .import_directory
            .as_ref()
            .and_then(|directory| directory.shared_worktrees())
            .ok_or(ErrorCode::UnsupportedCapability)?;
        let created = tokio::task::spawn_blocking(move || {
            provisioning.create(&input, &chrono::Utc::now().to_rfc3339())
        })
        .await
        .map_err(|_| ErrorCode::RegistryIo)?
        .map_err(|_| ErrorCode::InvalidMessage)?;
        request.config.cwd = created.workspace.cwd;
        Ok(created.workspace.workspace_id)
    }

    pub(super) async fn cleanup_created_worktree(
        &mut self,
        id: &str,
        workspace: &str,
    ) -> Result<(), ErrorCode> {
        // A failed registration may retain a native writer when its first close failed.
        self.manager
            .close(id)
            .await
            .map_err(|error| super::map_manager(&error))?;
        let provisioning = self
            .import_directory
            .as_ref()
            .and_then(|directory| directory.shared_worktrees())
            .ok_or(ErrorCode::UnsupportedCapability)?;
        let workspace = workspace.to_owned();
        tokio::task::spawn_blocking(move || {
            provisioning.archive(&workspace, &chrono::Utc::now().to_rfc3339())
        })
        .await
        .map_err(|_| ErrorCode::RegistryIo)?
        .map_err(|_| ErrorCode::RegistryIo)
    }

    pub(super) fn arm_auto_archive(
        &mut self,
        id: &str,
        workspace: &str,
        created_worktree: bool,
    ) -> Result<(), ErrorCode> {
        if created_worktree {
            let provisioning = self
                .import_directory
                .as_ref()
                .and_then(|directory| directory.shared_worktrees())
                .ok_or(ErrorCode::UnsupportedCapability)?;
            self.manager.auto_archive_worktree_on_finish(
                id.to_owned(),
                workspace.to_owned(),
                provisioning,
            );
        } else {
            self.manager.auto_archive_on_finish(id.to_owned());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
