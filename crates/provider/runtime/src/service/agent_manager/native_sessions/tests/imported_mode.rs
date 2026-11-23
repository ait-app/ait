use super::*;
use provider_core::ports::agent_session::{AgentResumePurpose, AgentSession, AgentSessionSpec};

/// An adapter that optionally overrides the mode of adopted sessions.
#[derive(Debug)]
struct ModeClient {
    provider: &'static str,
    mode: Option<&'static str>,
}

#[async_trait::async_trait]
impl AgentClient for ModeClient {
    fn provider(&self) -> &str {
        self.provider
    }

    fn imported_session_mode(&self) -> &'static str {
        self.mode.unwrap_or("read-only")
    }

    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        Ok(true)
    }

    async fn create_session(
        &self,
        _spec: &AgentSessionSpec,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        unreachable!("adopting history must not create a session")
    }

    async fn resume_session(
        &self,
        _handle: &AgentPersistenceHandle,
        _spec: &AgentSessionSpec,
        _purpose: AgentResumePurpose,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        unreachable!("adopting history must not resume a session")
    }
}

fn history() -> SessionHistory {
    SessionHistory {
        resume_metadata: std::collections::BTreeMap::new(),
        parent_id: None,
        descriptor: SessionDescriptor {
            provider_id: "adopting".to_owned(),
            provider_label: "Adopting".to_owned(),
            provider_handle_id: "native-1".to_owned(),
            cwd: "/project".to_owned(),
            title: None,
            first_prompt_preview: None,
            last_prompt_preview: None,
            last_activity_at: "2026-11-23T00:00:00Z".to_owned(),
        },
        created_at: "2026-11-23T00:00:00Z".to_owned(),
        config: domain::agent_runtime::StoredAgentConfig::default(),
        active: false,
        entries: Vec::new(),
    }
}

fn adopted_mode(manager: &AgentManager, provider: &str, configured: Option<&str>) -> String {
    let mut record = super::super::super::tests::stored_record();
    record.provider = provider.to_owned();
    record.config = Some(domain::agent_runtime::StoredAgentConfig {
        mode_id: configured.map(str::to_owned),
        ..Default::default()
    });
    manager.apply_history(&mut record, &history());
    record.runtime_info.unwrap().mode_id.unwrap()
}

#[test]
fn adopted_sessions_without_a_configured_mode_use_the_adapter_mode() {
    let mut manager = AgentManager::new(Box::new(
        super::super::super::tests::MemoryRegistry::default(),
    ));
    for (provider, mode) in [("adopting", Some("default")), ("plain", None)] {
        manager
            .register_client(Box::new(ModeClient { provider, mode }))
            .unwrap();
    }

    assert_eq!(adopted_mode(&manager, "adopting", None), "default");
    assert_eq!(adopted_mode(&manager, "plain", None), "read-only");
    assert_eq!(adopted_mode(&manager, "unregistered", None), "read-only");
    assert_eq!(adopted_mode(&manager, "adopting", Some("plan")), "plan");
}
