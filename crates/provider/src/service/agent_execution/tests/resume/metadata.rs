use std::collections::BTreeMap;
use std::sync::Mutex;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};

use super::*;
use crate::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionFuture,
    AgentSessionSpec,
};
use crate::ports::native_history::{SessionDescriptor, SessionHistory};

#[derive(Debug)]
struct MetadataClient {
    verified: BTreeMap<String, Value>,
    resumed: Arc<Mutex<Option<(AgentPersistenceHandle, AgentSessionSpec)>>>,
}

impl AgentClient for MetadataClient {
    fn provider(&self) -> &'static str {
        "metadata-test"
    }

    fn validate_config(&self, _config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        Ok(())
    }

    fn is_available(&self) -> AgentSessionFuture<'_, bool> {
        Box::pin(async { Ok(true) })
    }

    fn inspect_session<'a>(
        &'a self,
        handle: &'a AgentPersistenceHandle,
        cwd: &'a str,
    ) -> AgentSessionFuture<'a, SessionHistory> {
        Box::pin(async move {
            Ok(SessionHistory {
                resume_metadata: self.verified.clone(),
                parent_id: None,
                descriptor: SessionDescriptor {
                    provider_id: self.provider().into(),
                    provider_label: "Metadata test".into(),
                    provider_handle_id: handle.session_id.clone(),
                    cwd: cwd.into(),
                    title: None,
                    first_prompt_preview: None,
                    last_prompt_preview: None,
                    last_activity_at: "2026-10-20T00:00:00Z".into(),
                },
                created_at: "2026-10-20T00:00:00Z".into(),
                config: StoredAgentConfig {
                    model: Some("native-model".into()),
                    ..StoredAgentConfig::default()
                },
                active: false,
                entries: Vec::new(),
            })
        })
    }

    fn create_session<'a>(
        &'a self,
        _spec: &'a AgentSessionSpec,
    ) -> AgentSessionFuture<'a, Box<dyn AgentSession>> {
        Box::pin(async { Err(AgentSessionError::Unavailable) })
    }

    fn resume_session<'a>(
        &'a self,
        handle: &'a AgentPersistenceHandle,
        spec: &'a AgentSessionSpec,
        purpose: AgentResumePurpose,
    ) -> AgentSessionFuture<'a, Box<dyn AgentSession>> {
        Box::pin(async move {
            assert_eq!(purpose, AgentResumePurpose::Interactive);
            *self.resumed.lock().unwrap() = Some((handle.clone(), spec.clone()));
            // Stop at the native writer boundary so persistence cannot mask the supplied metadata.
            Err(AgentSessionError::Unavailable)
        })
    }
}

#[tokio::test]
async fn unknown_resume_replaces_metadata_before_opening_a_writer_and_preserves_overrides() {
    for verified in [
        BTreeMap::new(),
        BTreeMap::from([("providerFact".into(), json!("verified"))]),
    ] {
        let fixture = Fixture::new();
        let resumed = Arc::new(Mutex::new(None));
        let client = MetadataClient {
            verified: verified.clone(),
            resumed: resumed.clone(),
        };
        let (execution, registry) = worker_with_client(
            &fixture,
            model::creation::Creations::default(),
            Box::new(client),
        );
        let response = execution
            .execute(
                "agent.resume.request",
                json!({"handle":{"provider":"metadata-test","sessionId":"external",
                    "nativeHandle":"native-external","metadata":{"cwd":fixture.cwd,
                        "staleFact":true,"providerFact":"unverified","model":"metadata-model",
                        "systemPrompt":"metadata prompt"}},"overrides":{"model":"override-model"}}),
            )
            .await;

        assert!(response.is_err());
        let (handle, spec) = resumed
            .lock()
            .unwrap()
            .take()
            .expect("native resume reached");
        assert_eq!(handle.metadata, Some(verified));
        assert_eq!(handle.native_handle, Some(json!("native-external")));
        assert_eq!(spec.cwd, fixture.cwd.to_str().unwrap());
        assert_eq!(spec.config.model.as_deref(), Some("override-model"));
        assert_eq!(
            spec.config.system_prompt.as_deref(),
            Some("metadata prompt")
        );
        assert!(registry.list().unwrap().is_empty());
        execution.shutdown().await.unwrap();
    }
}
