use std::path::Path;
use std::sync::Arc;

use filesystem::files::local::files::LocalFiles;
use filesystem::files::service::files::Files;
use filesystem::forge::local::forge::LocalForge;
use filesystem::forge::service::forge::Forge;
use filesystem::git::local::{checkout::LocalCheckout, git_fetch::LocalGitFetch};
use filesystem::git::service::{checkout::Checkout, git_fetch::GitFetch};

/// Install a complete filesystem service with all storage confined to the test `root`.
pub(super) fn service(root: &Path) -> filesystem::Service {
    filesystem::Service::new(filesystem::Dependencies {
        checkout: Checkout::new(Box::new(LocalCheckout::new(root.join("worktrees")))),
        forge: Forge::new(Box::new(LocalForge::new())),
        files: Files::new(Box::new(LocalFiles::new(root.to_owned(), root))),
        git_fetch: GitFetch::new(Arc::new(LocalGitFetch::new(root.join("worktrees")))),
    })
}
