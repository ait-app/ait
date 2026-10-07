use std::path::Path;
use std::sync::{Arc, Mutex};

use file::storage::project_config::LocalProjectConfigStore;
use file::storage::project_icon::LocalProjectIconStore;
use file::storage::registry::{FileBackedProjectRegistry, FileBackedWorkspaceRegistry};
use filesystem::local::{
    checkout::LocalCheckout, files::LocalFiles, forge::LocalForge, git_fetch::LocalGitFetch,
    github_projects::LocalGithubProjects, provisioning::LocalDirectorySource, skills::LocalSkills,
    worktrees::LocalManagedWorktrees,
};
use filesystem::service::{
    checkout::Checkout, files::Files, forge::Forge, git_fetch::GitFetch,
    github_projects::GithubProjects, skills::Skills, workspace_recovery::WorkspaceRecovery,
    worktrees::Worktrees,
};
use metadata::service::directory::{Directory, DirectoryDependencies};

/// Install a complete filesystem service with all storage confined to the test `root`.
pub(super) fn service(root: &Path) -> filesystem::Service {
    let projects = FileBackedProjectRegistry::new(root.join("projects.json"));
    let workspaces = FileBackedWorkspaceRegistry::new(root.join("workspaces.json"));
    let managed = LocalManagedWorktrees::new(root.join("worktrees"));
    let directory = Directory::new(DirectoryDependencies {
        projects: Box::new(projects.clone()),
        workspaces: Box::new(workspaces.clone()),
        source: Box::new(LocalDirectorySource),
        config_store: Box::new(LocalProjectConfigStore),
        icon_store: Box::new(LocalProjectIconStore::new(root.join("icons"))),
        server_id: "stable".into(),
    });
    let worktrees = Worktrees::new(
        Box::new(projects.clone()),
        Box::new(workspaces.clone()),
        Box::new(managed.clone()),
        "stable".into(),
    );
    let skills = LocalSkills::new(
        &root.join("skill-source"),
        &[
            root.join("codex-skills"),
            root.join("claude-skills"),
            root.join("opencode-skills"),
        ],
        &root.join("skill-state"),
    )
    .unwrap();
    filesystem::Service::new(filesystem::Dependencies {
        checkout: Checkout::new(Box::new(LocalCheckout::new(root.join("worktrees")))),
        forge: Forge::new(Box::new(LocalForge::new())),
        files: Files::new(Box::new(LocalFiles::new(root.to_owned(), root))),
        git_fetch: GitFetch::new(Arc::new(LocalGitFetch::new(root.join("worktrees")))),
        github_projects: GithubProjects::new(
            Arc::new(directory),
            Box::new(LocalGithubProjects::new()),
        ),
        worktrees: Arc::new(Mutex::new(worktrees)),
        workspace_recovery: WorkspaceRecovery::new(
            Box::new(workspaces),
            Box::new(projects),
            Box::new(managed),
        ),
        skills: Skills::new(Box::new(skills)),
    })
}
