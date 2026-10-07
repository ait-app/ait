use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::ports::{environment::AgentEnvironment, native_history::ListOptions};
use crate::protocol::prompt::AgentPrompt;
use serde_json::json;

#[derive(Debug)]
struct MinimalSession;

impl AgentSession for MinimalSession {
    fn provider(&self) -> &'static str {
        "minimal"
    }
    fn runtime_info(&mut self) -> AgentSessionFuture<'_, StoredAgentRuntimeInfo> {
        Box::pin(async { Err(AgentSessionError::Unavailable) })
    }
    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        None
    }
    fn close(&mut self) -> AgentSessionFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
}

#[derive(Debug)]
struct MinimalClient {
    available: Result<bool, AgentSessionError>,
    created: AtomicUsize,
}

impl MinimalClient {
    fn new(available: Result<bool, AgentSessionError>) -> Self {
        Self {
            available,
            created: AtomicUsize::new(0),
        }
    }
}

impl AgentClient for MinimalClient {
    fn provider(&self) -> &'static str {
        "minimal"
    }
    fn is_available(&self) -> AgentSessionFuture<'_, bool> {
        Box::pin(async { self.available })
    }
    fn create_session<'a>(
        &'a self,
        _: &'a AgentSessionSpec,
    ) -> AgentSessionFuture<'a, Box<dyn AgentSession>> {
        self.created.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(Box::new(MinimalSession) as Box<dyn AgentSession>) })
    }
    fn resume_session<'a>(
        &'a self,
        _: &'a AgentPersistenceHandle,
        _: &'a AgentSessionSpec,
        _: AgentResumePurpose,
    ) -> AgentSessionFuture<'a, Box<dyn AgentSession>> {
        Box::pin(async { Err(AgentSessionError::Unavailable) })
    }
}

fn spec() -> AgentSessionSpec {
    AgentSessionSpec {
        provider: "minimal".into(),
        cwd: "/repo".into(),
        config: StoredAgentConfig::default(),
    }
}

fn handle() -> AgentPersistenceHandle {
    AgentPersistenceHandle {
        provider: "minimal".into(),
        session_id: "native".into(),
        native_handle: None,
        metadata: None,
    }
}

async fn unavailable<T>(operation: AgentSessionFuture<'_, T>) {
    assert_eq!(operation.await.err(), Some(AgentSessionError::Unavailable));
}

#[tokio::test]
async fn optional_session_controls_fail_without_claiming_unsupported_authority() {
    let mut session = MinimalSession;
    assert!(!session.pending_foreground());
    assert_eq!(session.cancel_pending().await, Ok(()));
    assert!(session.subagents().is_empty());
    assert!(session.pending_permissions().is_empty());
    assert_eq!(session.poll_turn().unwrap(), None);
    let response = json!({"approved":true});
    assert!(
        session
            .prepare_permission_response("stale", &response)
            .unwrap()
            .is_none()
    );
    assert!(
        session
            .permission_config_patch("stale", &response)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        session.respond_permission("stale", &response).await,
        Err(AgentSessionError::Rejected)
    );
    let prompt = AgentPrompt::text("hello");
    assert_eq!(
        session.out_of_band(&prompt).await,
        Err(AgentSessionError::Rejected)
    );
    unavailable(session.start_turn("hello", &StoredAgentConfig::default())).await;
    unavailable(session.cancel_turn("turn")).await;
    assert_eq!(
        session.steer_turn("turn", "hello").await,
        Err(AgentSessionError::Rejected)
    );
}

#[tokio::test]
async fn legacy_input_validates_the_entire_prompt_before_delegating() {
    let mut session = MinimalSession;
    let config = StoredAgentConfig::default();
    unavailable(session.start_input(&AgentPrompt::text("plain"), &config)).await;
    assert_eq!(
        session
            .steer_input("turn", &AgentPrompt::text("plain"))
            .await,
        Err(AgentSessionError::Rejected)
    );
    for prompt in [
        AgentPrompt::default(),
        AgentPrompt {
            text: "with output constraint".into(),
            output_schema: Some(json!({"type":"object"})),
            ..Default::default()
        },
        AgentPrompt {
            text: "with input identity".into(),
            client_message_id: Some("input-1".into()),
            ..Default::default()
        },
    ] {
        assert_eq!(
            session.start_input(&prompt, &config).await,
            Err(AgentSessionError::Rejected)
        );
        assert_eq!(
            session.steer_input("turn", &prompt).await,
            Err(AgentSessionError::Rejected)
        );
    }
}

#[tokio::test]
async fn unsupported_environment_is_rejected_before_session_creation() {
    let client = MinimalClient::new(Ok(true));
    let spec = spec();
    let environment =
        AgentEnvironment::try_from(BTreeMap::from([("FIXTURE".into(), "value".into())])).unwrap();
    assert_eq!(
        client
            .create_session_with_environment(&spec, &environment)
            .await
            .err(),
        Some(AgentSessionError::Rejected)
    );
    assert_eq!(client.created.load(Ordering::SeqCst), 0);
    let mut session = client
        .create_session_with_environment(&spec, &AgentEnvironment::default())
        .await
        .unwrap();
    assert_eq!(session.provider(), "minimal");
    assert_eq!(client.created.load(Ordering::SeqCst), 1);
    session.close().await.unwrap();
}

#[tokio::test]
async fn default_discovery_and_control_contracts_report_unavailable() {
    let client = MinimalClient::new(Ok(true));
    let spec = spec();
    let handle = handle();
    assert!(client.supports_history_replay());
    assert!(!client.handles_out_of_band("/unknown"));
    assert!(client.persisted_permissions(&handle).is_empty());
    assert_eq!(
        client.settings(&spec.config),
        json!({"availableModes":[],"features":[],"capabilities":{}})
    );
    assert!(client.draft_features(&spec).await.unwrap().is_empty());
    assert_eq!(
        client.validate_config(&spec.config),
        Err(AgentSessionError::Unavailable)
    );
    unavailable(client.validate_selection(&spec)).await;
    unavailable(client.generate_summary(&spec, "title", &json!({}))).await;
    unavailable(client.diagnostic()).await;
    unavailable(client.usage()).await;
    unavailable(client.commands(&spec)).await;
    unavailable(client.subagents(&spec.cwd)).await;
    unavailable(client.rewind(&handle, &spec, "message")).await;
    unavailable(client.rewind_files(&handle, &spec, "message")).await;
    unavailable(client.discover(&spec.cwd)).await;
    unavailable(client.history(&handle, &spec.cwd)).await;
    unavailable(client.list_sessions(&ListOptions {
        cwd: None,
        scan_limit: 10,
    }))
    .await;
    unavailable(client.inspect_session(&handle, &spec.cwd)).await;
    for availability in [Ok(false), Err(AgentSessionError::Failed)] {
        let client = MinimalClient::new(availability);
        let expected = availability.err().unwrap_or(AgentSessionError::Unavailable);
        assert_eq!(client.draft_features(&spec).await.unwrap_err(), expected);
        assert_eq!(client.created.load(Ordering::SeqCst), 0);
    }
}
