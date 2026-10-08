use domain::workspace::lifecycle::WorkspaceLifecycleError;
use domain::workspace::records::PersistedProjectKind;
use model::workspace::lifecycle::ProjectRegistration;
use std::sync::{Arc, Mutex};

use super::*;
use crate::forge::ports::github_projects::{
    GithubProjectsError, GithubProjectsRuntime, GithubRepository, GithubRepositoryVisibility,
};

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
    let directory = directory();
    assert_eq!(
        crate::forge::rpc::github_projects::execute(&directory, "unknown", serde_json::json!({})),
        Err(crate::support::error::ErrorCode::MethodNotFound)
    );
    for params in [
        serde_json::json!({"repo":"ab","targetDirectory":"/tmp/projects"}),
        serde_json::json!({"repo":"owner/repo","targetDirectory":" "}),
    ] {
        assert_eq!(
            crate::forge::rpc::github_projects::execute(
                &directory,
                "project.github.clone.request",
                params
            ),
            Err(crate::support::error::ErrorCode::InvalidMessage)
        );
    }
}
#[derive(Debug, Default)]
struct Registration {
    calls: Mutex<Vec<(String, String)>>,
}

impl ProjectRegistration for Registration {
    fn register_project(
        &self,
        path: &str,
        timestamp: &str,
    ) -> Result<PersistedProjectRecord, WorkspaceLifecycleError> {
        self.calls
            .lock()
            .unwrap()
            .push((path.to_owned(), timestamp.to_owned()));
        if path.contains("missing") {
            return Err(WorkspaceLifecycleError {
                message: "directory not found".to_owned(),
            });
        }
        Ok(PersistedProjectRecord {
            project_id: "prj_test".to_owned(),
            root_path: path.to_owned(),
            kind: PersistedProjectKind::Git,
            display_name: "repo".to_owned(),
            project_key: Some("remote:github.com/example/repo".to_owned()),
            custom_name: None,
            custom_icon_revision: None,
            created_at: timestamp.to_owned(),
            updated_at: timestamp.to_owned(),
            archived_at: None,
        })
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

fn directory() -> GithubProjects {
    GithubProjects::new(
        Arc::new(Registration::default()),
        Box::new(Github::default()),
    )
}

#[test]
fn repository_search_preserves_availability_and_authentication_error_categories() {
    let mut directory = directory();
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
        directory.github = Box::new(Github {
            search_error: Some(error),
        });
        let result = crate::forge::rpc::github_projects::execute(
            &directory,
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
    let directory = directory();
    let outcome = directory.clone_github_project(
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
        directory
            .search_github_repositories("repo", 10)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn github_clone_rejects_unsafe_input_without_launching_or_registering() {
    let directory = directory();
    for repo in [
        "owner/../repo",
        "https://evil.example/owner/repo",
        "owner/repo",
    ] {
        let outcome = directory.clone_github_project(repo, None, "/tmp/projects", "time");
        assert!(outcome.checkout_path.is_none(), "{repo}");
        assert!(outcome.project.is_none(), "{repo}");
        assert!(outcome.error.is_some(), "{repo}");
    }
}

#[test]
fn github_clone_preserves_completed_checkout_on_registration_failure() {
    let directory = directory();

    let outcome = directory.clone_github_project(
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
    assert!(outcome.error.is_some());
}

#[test]
fn github_clone_reports_planned_checkout_path_when_clone_fails() {
    let directory = directory();
    let outcome = directory.clone_github_project(
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
fn clone_passes_the_completed_checkout_and_timestamp_to_shared_registration() {
    let registration = Arc::new(Registration::default());
    let service = GithubProjects::new(registration.clone(), Box::new(Github::default()));
    let outcome = service.clone_github_project(
        "owner/repo",
        Some(GithubCloneProtocol::Https),
        "/projects",
        "now",
    );
    assert!(outcome.error.is_none());
    assert_eq!(
        registration.calls.lock().unwrap().as_slice(),
        &[("/projects/repo".to_owned(), "now".to_owned())]
    );
    assert_eq!(outcome.project.unwrap().created_at, "now");
    let outcome = service.clone_github_project(
        "owner/exists",
        Some(GithubCloneProtocol::Https),
        "/projects",
        "later",
    );
    assert!(outcome.error.is_some());
    assert_eq!(registration.calls.lock().unwrap().len(), 1);
    let outcome = service.clone_github_project("owner/../unsafe", None, "/projects", "later");
    assert!(outcome.error.is_some());
    assert_eq!(registration.calls.lock().unwrap().len(), 1);
}
