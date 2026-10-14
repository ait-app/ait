use model::workspace::naming::WorkspaceBranchNamer;

use super::{
    CheckoutRuntime, LocalCheckout, git_optional, git_write, require_git_directory,
    validate_branch_slug,
};

impl WorkspaceBranchNamer for LocalCheckout {
    fn rename(&self, cwd: &str, expected: &str, desired: &str) -> Option<String> {
        let status = self.status(cwd).ok()?;
        if !status.is_managed_worktree
            || status.current_branch.as_deref() != Some(expected)
            || status.upstream_ref.is_some()
            || expected == desired
        {
            return None;
        }
        let cwd = require_git_directory(cwd).ok()?;
        let desired = validate_branch_slug(desired).ok()?;
        for suffix in 0..50 {
            let candidate = if suffix == 0 {
                desired.clone()
            } else {
                format!("{desired}-{}", suffix + 1)
            };
            if git_optional(
                &cwd,
                &["show-ref", "--verify", &format!("refs/heads/{candidate}")],
            )
            .ok()?
            .is_some()
            {
                continue;
            }
            if git_optional(&cwd, &["symbolic-ref", "--quiet", "--short", "HEAD"])
                .ok()?
                .as_deref()
                != Some(expected)
            {
                return None;
            }
            // Supplying the expected old name prevents a concurrent checkout from renaming its new branch.
            git_write(&cwd, &["branch", "-m", expected, &candidate]).ok()?;
            return Some(candidate);
        }
        None
    }
}
