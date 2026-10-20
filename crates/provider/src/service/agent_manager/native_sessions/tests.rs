use std::path::PathBuf;

use domain::agent_runtime::registry::AgentRuntimeRegistry;

use super::*;
use crate::ports::native_history::SessionDescriptor;

#[test]
fn recent_query_matches_all_display_fields_and_paths_are_canonical_directories() {
    let descriptor = SessionDescriptor {
        provider_id: "codex".to_owned(),
        provider_label: "Codex".to_owned(),
        provider_handle_id: "Native".to_owned(),
        cwd: "/project".to_owned(),
        title: Some("Title".to_owned()),
        first_prompt_preview: Some("First".to_owned()),
        last_prompt_preview: Some("Last".to_owned()),
        last_activity_at: "2026-09-25T00:00:00Z".to_owned(),
    };
    for query in ["", "native", "project", "title", "first", "last"] {
        assert!(matches_query(&descriptor, query));
    }
    assert!(!matches_query(&descriptor, "absent"));
    let root = tempfile::tempdir().unwrap();
    assert_eq!(
        canonical(root.path().to_str().unwrap()).unwrap(),
        root.path().canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(canonical("relative"), Err(ErrorCode::InvalidMessage));
    let file = root.path().join("file");
    std::fs::write(&file, b"file").unwrap();
    assert_eq!(
        canonical(file.to_str().unwrap()),
        Err(ErrorCode::WorkingDirectoryUnavailable)
    );
    let missing = root.path().join("deleted-workspace");
    assert_eq!(
        canonical(missing.to_str().unwrap()),
        Err(ErrorCode::WorkingDirectoryUnavailable)
    );
    assert!(
        !missing.exists(),
        "validation must not recreate a workspace"
    );
}

#[tokio::test]
async fn archived_internal_sessions_stay_hidden_from_native_rediscovery() {
    let registry = super::super::tests::MemoryRegistry::default();
    let mut sessions = Vec::new();
    for (internal, archived) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut record = super::super::tests::stored_record();
        record.id = format!("internal-{internal}-archived-{archived}");
        record.internal = internal;
        record.archived_at = archived.then(|| "2026-10-12T00:00:00Z".into());
        record.persistence = Some(AgentPersistenceHandle {
            provider: "working".into(),
            session_id: record.id.clone(),
            native_handle: None,
            metadata: None,
        });
        registry.upsert(&record).unwrap();
        sessions.push(SessionDescriptor {
            provider_id: "working".into(),
            provider_label: "Working".into(),
            provider_handle_id: record.id,
            cwd: "/project".into(),
            title: None,
            first_prompt_preview: None,
            last_prompt_preview: None,
            last_activity_at: "2026-10-07T00:00:00Z".into(),
        });
    }
    let mut manager = AgentManager::new(Box::new(registry));
    manager
        .register_client(Box::new(ImportClient {
            provider: "working",
            supported: true,
            available: Ok(true),
            sessions: Ok(sessions),
            expected_cwd: None,
            remove_cwd: None,
        }))
        .unwrap();
    let result = manager
        .recent_sessions(serde_json::from_value(json!({})).unwrap())
        .await
        .unwrap();
    assert_eq!(result["filteredAlreadyImportedCount"], 3);
    assert_eq!(result["entries"].as_array().unwrap().len(), 1);
    assert_eq!(
        result["entries"][0]["providerHandleId"],
        "internal-false-archived-true"
    );
}

#[tokio::test]
async fn recent_sessions_wait_for_the_shared_history_budget() {
    use std::task::Poll;

    let mut manager = AgentManager::new(Box::new(super::super::tests::MemoryRegistry::default()));
    manager
        .register_client(Box::new(ImportClient {
            provider: "working",
            supported: true,
            available: Ok(true),
            sessions: Ok(Vec::new()),
            expected_cwd: None,
            remove_cwd: None,
        }))
        .unwrap();
    let held = manager
        .history_budget
        .clone()
        .acquire_many_owned(8)
        .await
        .unwrap();
    let listing = manager.recent_sessions(serde_json::from_value(json!({})).unwrap());
    tokio::pin!(listing);

    assert!(matches!(futures_util::poll!(&mut listing), Poll::Pending));
    drop(held);
    assert!(listing.await.is_ok());
}

#[derive(Debug)]
struct ImportClient {
    provider: &'static str,
    supported: bool,
    available: Result<bool, AgentSessionError>,
    sessions: Result<Vec<SessionDescriptor>, AgentSessionError>,
    expected_cwd: Option<String>,
    remove_cwd: Option<PathBuf>,
}

impl AgentClient for ImportClient {
    fn provider(&self) -> &str {
        self.provider
    }

    fn supports_session_import(&self) -> bool {
        self.supported
    }

    fn is_available(&self) -> crate::ports::agent_session::AgentSessionFuture<'_, bool> {
        assert!(self.supported);
        Box::pin(async { self.available })
    }

    fn list_sessions<'a>(
        &'a self,
        options: &'a ListOptions,
    ) -> crate::ports::agent_session::AgentSessionFuture<'a, Vec<SessionDescriptor>> {
        assert_eq!(self.available, Ok(true));
        if let Some(expected) = &self.expected_cwd {
            assert_eq!(options.cwd.as_ref(), Some(expected));
        }
        if let Some(cwd) = &self.remove_cwd {
            std::fs::remove_dir(cwd).expect("requested cwd should exist until provider listing");
        }
        Box::pin(async { self.sessions.clone() })
    }

    fn create_session<'a>(
        &'a self,
        _spec: &'a crate::ports::agent_session::AgentSessionSpec,
    ) -> crate::ports::agent_session::AgentSessionFuture<
        'a,
        Box<dyn crate::ports::agent_session::AgentSession>,
    > {
        unreachable!("listing must not create a session")
    }

    fn resume_session<'a>(
        &'a self,
        _handle: &'a AgentPersistenceHandle,
        _spec: &'a crate::ports::agent_session::AgentSessionSpec,
        _purpose: crate::ports::agent_session::AgentResumePurpose,
    ) -> crate::ports::agent_session::AgentSessionFuture<
        'a,
        Box<dyn crate::ports::agent_session::AgentSession>,
    > {
        unreachable!("listing must not resume a session")
    }
}

#[tokio::test]
async fn recent_sessions_skip_absent_and_unsupported_but_preserve_real_errors_and_rows() {
    let mut manager = AgentManager::new(Box::new(super::super::tests::MemoryRegistry::default()));
    for (provider, supported, available, sessions) in [
        (
            "unsupported",
            false,
            Ok(true),
            Err(AgentSessionError::Unavailable),
        ),
        (
            "missing",
            true,
            Ok(false),
            Err(AgentSessionError::Unavailable),
        ),
        (
            "probe-failed",
            true,
            Err(AgentSessionError::Failed),
            Ok(Vec::new()),
        ),
        (
            "read-failed",
            true,
            Ok(true),
            Err(AgentSessionError::Failed),
        ),
        ("empty", true, Ok(true), Ok(Vec::new())),
        (
            "working",
            true,
            Ok(true),
            Ok(vec![SessionDescriptor {
                provider_id: "working".to_owned(),
                provider_label: "Working".to_owned(),
                provider_handle_id: "session-1".to_owned(),
                cwd: "/project".to_owned(),
                title: None,
                first_prompt_preview: None,
                last_prompt_preview: None,
                last_activity_at: "2026-10-07T00:00:00Z".to_owned(),
            }]),
        ),
    ] {
        manager
            .register_client(Box::new(ImportClient {
                provider,
                supported,
                available,
                sessions,
                expected_cwd: None,
                remove_cwd: None,
            }))
            .unwrap();
    }
    let result = manager
        .recent_sessions(serde_json::from_value(json!({})).unwrap())
        .await
        .unwrap();
    assert_eq!(result["entries"].as_array().unwrap().len(), 1);
    assert_eq!(result["entries"][0]["providerId"], "working");
    let errors = result["providerErrors"].as_array().unwrap();
    assert_eq!(errors.len(), 2);
    assert_eq!(errors[0]["provider"], "probe-failed");
    assert_eq!(errors[1]["provider"], "read-failed");
}

#[cfg(unix)]
#[tokio::test]
async fn recent_sessions_keep_non_pi_provider_filters_canonical() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("project");
    let linked_project = directory.path().join("linked-project");
    std::fs::create_dir(&project).unwrap();
    symlink(&project, &linked_project).unwrap();
    let lookup_cwd = linked_project.to_str().unwrap().to_owned();
    let mut manager = AgentManager::new(Box::new(super::super::tests::MemoryRegistry::default()));
    manager
        .register_client(Box::new(ImportClient {
            provider: "working",
            supported: true,
            available: Ok(true),
            sessions: Ok(vec![SessionDescriptor {
                provider_id: "working".to_owned(),
                provider_label: "Working".to_owned(),
                provider_handle_id: "session".to_owned(),
                cwd: lookup_cwd.clone(),
                title: None,
                first_prompt_preview: None,
                last_prompt_preview: None,
                last_activity_at: "2026-10-07T00:00:00Z".to_owned(),
            }]),
            expected_cwd: Some(project.to_str().unwrap().to_owned()),
            remove_cwd: None,
        }))
        .unwrap();

    let result = manager
        .recent_sessions(
            serde_json::from_value(json!({"cwd":lookup_cwd})).expect("valid recent request"),
        )
        .await
        .unwrap();

    assert_eq!(result["entries"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn recent_sessions_reject_candidates_if_the_requested_cwd_vanishes_during_listing() {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("project");
    let missing_session_cwd = directory.path().join("missing-session-cwd");
    std::fs::create_dir(&project).unwrap();
    let lookup_cwd = project.to_str().unwrap().to_owned();
    let mut manager = AgentManager::new(Box::new(super::super::tests::MemoryRegistry::default()));
    manager
        .register_client(Box::new(ImportClient {
            provider: "working",
            supported: true,
            available: Ok(true),
            sessions: Ok(vec![SessionDescriptor {
                provider_id: "working".to_owned(),
                provider_label: "Working".to_owned(),
                provider_handle_id: "session".to_owned(),
                cwd: missing_session_cwd.to_str().unwrap().to_owned(),
                title: None,
                first_prompt_preview: None,
                last_prompt_preview: None,
                last_activity_at: "2026-10-07T00:00:00Z".to_owned(),
            }]),
            expected_cwd: Some(lookup_cwd.clone()),
            remove_cwd: Some(project),
        }))
        .unwrap();

    let result = manager
        .recent_sessions(
            serde_json::from_value(json!({"cwd":lookup_cwd})).expect("valid recent request"),
        )
        .await
        .unwrap();

    assert!(result["entries"].as_array().unwrap().is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn recent_sessions_preserve_the_requested_directory_spelling_for_pi() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("project");
    let linked_project = directory.path().join("linked-project");
    std::fs::create_dir(&project).unwrap();
    symlink(&project, &linked_project).unwrap();
    let lookup_cwd = linked_project.to_str().unwrap().to_owned();
    let mut manager = AgentManager::new(Box::new(super::super::tests::MemoryRegistry::default()));
    manager
        .register_client(Box::new(ImportClient {
            provider: crate::local::pi::PROVIDER,
            supported: true,
            available: Ok(true),
            sessions: Ok(vec![SessionDescriptor {
                provider_id: crate::local::pi::PROVIDER.to_owned(),
                provider_label: "Pi".to_owned(),
                provider_handle_id: "session".to_owned(),
                cwd: lookup_cwd.clone(),
                title: None,
                first_prompt_preview: None,
                last_prompt_preview: None,
                last_activity_at: "2026-10-07T00:00:00Z".to_owned(),
            }]),
            expected_cwd: Some(lookup_cwd.clone()),
            remove_cwd: None,
        }))
        .unwrap();

    let result = manager
        .recent_sessions(
            serde_json::from_value(json!({"cwd":lookup_cwd})).expect("valid recent request"),
        )
        .await
        .unwrap();

    assert_eq!(result["entries"].as_array().unwrap().len(), 1);
}
