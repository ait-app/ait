use domain::storage::project::{
    ProjectConfigDocument, ProjectConfigRevision, ProjectConfigStoreError, ProjectConfigWrite,
    ProjectIcon, ProjectIconStoreError,
};
use domain::workspace::provisioning::{Checkout, DirectorySourceError, GithubRepositoryVisibility};
use domain::workspace::records::PersistedProjectKind;
use model::storage::project::{ProjectConfigStore, ProjectIconStore};
use model::workspace::provisioning::DirectorySource;
use persistence::storage::registry::FileBackedProjectRegistry;
use persistence::storage::registry::FileBackedWorkspaceRegistry;

use super::*;
use crate::rpc::ErrorCode;
use crate::rpc::github_projects::execute;
use crate::service::directory::DirectoryDependencies;

#[test]
fn direct_github_urls_preserve_the_requested_transport_and_reject_ambiguous_urls() {
    for url in [
        "https://github.com/owner/repo.git",
        "git@github.com:owner/repo.git",
        "ssh://git@github.com/owner/repo.git",
    ] {
        assert_eq!(
            normalize_clone_repository(url, None),
            Some(("repo".to_owned(), "owner/repo".to_owned(), url.to_owned(),))
        );
    }
    for url in [
        "",
        "ab",
        "owner/repo\nother",
        "https://github.com/owner/repo?ref=main",
        "https://github.com/owner/repo#readme",
        "ssh://git@github.com/owner/../repo",
    ] {
        assert!(normalize_clone_repository(url, None).is_none(), "{url}");
    }
    let (_root, service) = service();
    assert_eq!(
        execute(&service, "unknown", serde_json::json!({})),
        Err(ErrorCode::MethodNotFound)
    );
    for params in [
        serde_json::json!({"repo":"ab","targetDirectory":"/tmp/projects"}),
        serde_json::json!({"repo":"owner/repo","targetDirectory":" "}),
    ] {
        assert_eq!(
            execute(&service, "project.github.clone.request", params),
            Err(ErrorCode::InvalidMessage)
        );
    }
}
/// Local inspection stub: every path below `missing` is absent, everything else is a Git root.
#[derive(Debug)]
struct Source;

impl DirectorySource for Source {
    fn inspect(&self, path: &str) -> Result<Checkout, DirectorySourceError> {
        if path.contains("missing") {
            return Err(DirectorySourceError::NotFound);
        }
        Ok(Checkout {
            cwd: path.to_owned(),
            is_git: true,
            current_branch: Some("main".to_owned()),
            remote_url: Some("git@github.com:example/repo.git".to_owned()),
            worktree_root: Some(path.to_owned()),
            is_paseo_owned_worktree: false,
            main_repo_root: None,
        })
    }

    fn create_child(&self, _parent: &str, _name: &str) -> Result<String, DirectorySourceError> {
        Err(DirectorySourceError::Io)
    }

    fn remove_empty(&self, _path: &str) -> Result<(), DirectorySourceError> {
        Ok(())
    }

    fn equivalent(&self, left: &str, right: &str) -> bool {
        left == right
    }

    fn canonical(&self, path: &str) -> Result<String, DirectorySourceError> {
        Ok(path.to_owned())
    }
}

#[derive(Debug)]
struct NoConfig;

impl ProjectConfigStore for NoConfig {
    fn config_path(&self, root: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
        Ok(root.join("ait.json"))
    }

    fn read(&self, _root: &str) -> Result<ProjectConfigDocument, ProjectConfigStoreError> {
        Ok(ProjectConfigDocument {
            config: None,
            revision: None,
        })
    }

    fn write(
        &self,
        _root: &str,
        _config: &serde_json::Value,
        _expected_revision: Option<ProjectConfigRevision>,
    ) -> Result<ProjectConfigWrite, ProjectConfigStoreError> {
        Err(ProjectConfigStoreError::Write)
    }
}

#[derive(Debug)]
struct NoIcons;

impl ProjectIconStore for NoIcons {
    fn write_custom(&self, _project_id: &str, _bytes: &[u8]) -> Result<(), ProjectIconStoreError> {
        Err(ProjectIconStoreError::Invalid)
    }

    fn remove_custom(&self, _project_id: &str) -> Result<(), ProjectIconStoreError> {
        Ok(())
    }

    fn read_custom(&self, _project_id: &str) -> Result<Option<ProjectIcon>, ProjectIconStoreError> {
        Ok(None)
    }

    fn find_automatic(
        &self,
        _project_root: &str,
    ) -> Result<Option<ProjectIcon>, ProjectIconStoreError> {
        Ok(None)
    }
}

#[derive(Debug, Default)]
struct Github {
    search_error: Option<GithubProjectsError>,
}

impl GithubProjectsRuntime for Github {
    fn search_repositories(
        &self,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<GithubRepository>, GithubProjectsError> {
        if let Some(error) = self.search_error {
            return Err(error);
        }
        Ok(vec![GithubRepository {
            id: "R_1".to_owned(),
            name: "repo".to_owned(),
            name_with_owner: "owner/repo".to_owned(),
            description: None,
            visibility: GithubRepositoryVisibility::Public,
            updated_at: "2026-09-23T00:00:00Z".to_owned(),
            clone_url: "https://github.com/owner/repo".to_owned(),
        }])
    }

    fn checkout_path(
        &self,
        target_directory: &str,
        name: &str,
    ) -> Result<String, GithubProjectsError> {
        Ok(format!("{target_directory}/{name}"))
    }

    fn clone_repository(
        &self,
        _clone_url: &str,
        target_directory: &str,
        name: &str,
    ) -> Result<String, GithubProjectsError> {
        if name == "exists" {
            return Err(GithubProjectsError::TargetExists);
        }
        Ok(format!("{target_directory}/{name}"))
    }
}

/// Compose the service over a real Directory with file registries below a temporary root.
fn service() -> (tempfile::TempDir, GithubProjects) {
    let root = tempfile::tempdir().expect("registry root");
    let directory = Directory::new(DirectoryDependencies {
        projects: Box::new(FileBackedProjectRegistry::new(
            root.path().join("projects.json"),
        )),
        workspaces: Box::new(FileBackedWorkspaceRegistry::new(
            root.path().join("workspaces.json"),
        )),
        source: Box::new(Source),
        config_store: Box::new(NoConfig),
        icon_store: Box::new(NoIcons),
        server_id: "server-test".to_owned(),
    });
    (
        root,
        GithubProjects::new(directory, Box::new(Github::default())),
    )
}

#[test]
fn repository_search_preserves_availability_and_authentication_error_categories() {
    let (_root, mut service) = service();
    for (error, status, available, reason) in [
        (
            GithubProjectsError::CliMissing,
            "unavailable",
            false,
            Some("gh_missing"),
        ),
        (
            GithubProjectsError::Unauthenticated,
            "unauthenticated",
            false,
            None,
        ),
        (GithubProjectsError::SearchFailed, "error", true, None),
        (GithubProjectsError::InvalidTarget, "error", true, None),
        (GithubProjectsError::TargetExists, "error", true, None),
        (GithubProjectsError::CloneFailed, "error", true, None),
    ] {
        service.github = Box::new(Github {
            search_error: Some(error),
        });
        let result = execute(
            &service,
            "workspace.github.search_repositories.request",
            serde_json::json!({"query":"repo","limit":1}),
        )
        .unwrap();
        assert_eq!(result["status"], status);
        assert_eq!(result["available"], available);
        assert_eq!(result["reason"], serde_json::json!(reason));
        assert_eq!(result["error"], error.to_string());
        assert_eq!(result["repositories"], serde_json::json!([]));
    }
}
#[test]
fn github_clone_normalizes_repo_and_registers_project_without_workspace() {
    let (_root, service) = service();
    let outcome = service.clone_github_project(
        " owner/repo.git ",
        Some(GithubCloneProtocol::Ssh),
        "/tmp/projects",
        "2026-09-23T00:00:00Z",
    );

    assert_eq!(outcome.repo, "owner/repo");
    assert_eq!(outcome.checkout_path.as_deref(), Some("/tmp/projects/repo"));
    assert_eq!(
        outcome
            .project
            .as_ref()
            .map(|project| project.root_path.as_str()),
        Some("/tmp/projects/repo")
    );
    assert!(outcome.error.is_none());
    assert_eq!(
        service.directory.list_workspaces().unwrap(),
        Vec::new(),
        "cloning registers a Project without a Workspace"
    );
    assert_eq!(
        service
            .search_github_repositories("repo", 10)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn github_clone_rejects_unsafe_input_without_launching_or_registering() {
    let (_root, service) = service();
    for repo in [
        "owner/../repo",
        "https://evil.example/owner/repo",
        "owner/repo",
    ] {
        let outcome = service.clone_github_project(repo, None, "/tmp/projects", "time");
        assert!(outcome.checkout_path.is_none(), "{repo}");
        assert!(outcome.project.is_none(), "{repo}");
        assert!(outcome.error.is_some(), "{repo}");
    }
    assert!(service.directory.list_projects().unwrap().is_empty());
}

#[test]
fn github_clone_preserves_completed_checkout_on_registration_failure() {
    let (_root, service) = service();

    let outcome = service.clone_github_project(
        "owner/missing",
        Some(GithubCloneProtocol::Https),
        "/tmp/projects",
        "time",
    );

    assert_eq!(
        outcome.checkout_path.as_deref(),
        Some("/tmp/projects/missing")
    );
    assert!(outcome.project.is_none());
    assert_eq!(outcome.error.as_deref(), Some("directory not found"));
}

#[test]
fn github_clone_reports_planned_checkout_path_when_clone_fails() {
    let (_root, service) = service();
    let outcome = service.clone_github_project(
        "owner/exists",
        Some(GithubCloneProtocol::Https),
        "/tmp/projects",
        "time",
    );

    assert_eq!(
        outcome.checkout_path.as_deref(),
        Some("/tmp/projects/exists")
    );
    assert!(outcome.project.is_none());
    assert_eq!(
        outcome.error.as_deref(),
        Some("Checkout path already exists")
    );
}

#[test]
fn clone_registers_the_completed_checkout_in_the_shared_directory_once() {
    let (_root, service) = service();
    let outcome = service.clone_github_project(
        "owner/repo",
        Some(GithubCloneProtocol::Https),
        "/projects",
        "now",
    );
    assert!(outcome.error.is_none());
    let project = outcome.project.unwrap();
    assert_eq!(project.root_path, "/projects/repo");
    assert_eq!(project.kind, PersistedProjectKind::Git);
    assert_eq!(project.created_at, "now");
    assert_eq!(
        service.directory.list_projects().unwrap(),
        std::slice::from_ref(&project)
    );

    let reused = service.clone_github_project(
        "owner/repo",
        Some(GithubCloneProtocol::Https),
        "/projects",
        "later",
    );
    assert_eq!(reused.project.unwrap().project_id, project.project_id);
    for repo in ["owner/exists", "owner/../unsafe"] {
        let outcome = service.clone_github_project(
            repo,
            Some(GithubCloneProtocol::Https),
            "/projects",
            "later",
        );
        assert!(outcome.error.is_some(), "{repo}");
    }
    assert_eq!(service.directory.list_projects().unwrap().len(), 1);
}
