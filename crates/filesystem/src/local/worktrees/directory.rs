use model::workspace::worktrees::DirectoryGit;

use super::{
    LocalManagedWorktrees, READ_TIMEOUT, WRITE_TIMEOUT, WorktreeError, default_branch, git,
    local_branch_exists, resolve_base, validate_branch, validate_slug,
};

pub(super) fn prepare(cwd: &str, intent: &DirectoryGit) -> Result<(), WorktreeError> {
    let repository = LocalManagedWorktrees::repository(cwd)?;
    let cwd = &repository.source_cwd;
    let (branch, base) = match intent {
        DirectoryGit::BranchOff { branch, base } => {
            validate_slug(branch)?;
            validate_branch(cwd, branch)?;
            if local_branch_exists(cwd, branch) {
                return Err(WorktreeError::Invalid(format!(
                    "Branch already exists: {branch}"
                )));
            }
            let base = base.clone().map_or_else(|| default_branch(cwd), Ok)?;
            validate_reference(&base)?;
            (branch, Some(resolve_base(cwd, &base)?))
        }
        DirectoryGit::Checkout { branch } => {
            validate_reference(branch)?;
            (branch, None)
        }
    };
    let status = git(
        cwd,
        &["status", "--porcelain=v1", "--untracked-files=normal"],
        READ_TIMEOUT,
        &[0],
    )?;
    if !status.stdout.trim().is_empty() {
        return Err(WorktreeError::Invalid(
            "Working tree has uncommitted changes".to_owned(),
        ));
    }
    if let Some(base) = base {
        git(
            cwd,
            &["checkout", "-b", branch, "--no-track", &base],
            WRITE_TIMEOUT,
            &[0],
        )?;
    } else {
        let reference = resolve_base(cwd, branch)?;
        let local = reference.strip_prefix("refs/heads/");
        if let Some(local) = local {
            git(cwd, &["checkout", local], WRITE_TIMEOUT, &[0])?;
        } else if let Some(name) = reference.strip_prefix("refs/remotes/origin/") {
            validate_branch(cwd, name)?;
            git(
                cwd,
                &["checkout", "-b", name, "--track", &reference],
                WRITE_TIMEOUT,
                &[0],
            )?;
        } else {
            return Err(WorktreeError::UnknownBranch(branch.clone()));
        }
    }
    Ok(())
}

fn validate_reference(reference: &str) -> Result<(), WorktreeError> {
    if reference.is_empty()
        || reference.starts_with('-')
        || reference.contains("..")
        || reference.contains("@{")
        || !reference
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "._/-".contains(ch))
    {
        return Err(WorktreeError::Invalid(
            "Invalid branch reference".to_owned(),
        ));
    }
    Ok(())
}
