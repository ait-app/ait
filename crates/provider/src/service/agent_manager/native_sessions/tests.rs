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
    assert!(canonical("relative").is_err());
    let file = root.path().join("file");
    std::fs::write(&file, b"file").unwrap();
    assert!(canonical(file.to_str().unwrap()).is_err());
}

#[derive(Debug)]
struct ImportClient {
    provider: &'static str,
    supported: bool,
    available: Result<bool, AgentSessionError>,
    sessions: Result<Vec<SessionDescriptor>, AgentSessionError>,
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
        _options: &'a ListOptions,
    ) -> crate::ports::agent_session::AgentSessionFuture<'a, Vec<SessionDescriptor>> {
        assert_eq!(self.available, Ok(true));
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
