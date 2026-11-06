use std::sync::Mutex;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};

use super::*;
use crate::local::codex::CodexClient;
use crate::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionSpec,
};

/// Codex fixture behind the default Bot-session capabilities, as for `MiniMax`.
#[derive(Debug)]
struct Degraded {
    native: CodexClient,
    specs: Arc<Mutex<Vec<AgentSessionSpec>>>,
}

#[async_trait::async_trait]
impl AgentClient for Degraded {
    fn provider(&self) -> &'static str {
        "codex"
    }
    fn validate_config(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        self.native.validate_config(config)
    }
    fn settings(&self, config: &StoredAgentConfig) -> Value {
        self.native.settings(config)
    }
    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        self.native.is_available().await
    }
    async fn validate_selection(&self, spec: &AgentSessionSpec) -> Result<(), AgentSessionError> {
        self.native.validate_selection(spec).await
    }
    async fn create_session(
        &self,
        spec: &AgentSessionSpec,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        self.specs.lock().unwrap().push(spec.clone());
        self.native.create_session(spec).await
    }
    async fn resume_session(
        &self,
        handle: &AgentPersistenceHandle,
        spec: &AgentSessionSpec,
        purpose: AgentResumePurpose,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        self.native.resume_session(handle, spec, purpose).await
    }
}

fn degraded(
    fixture: &Fixture,
) -> (
    AgentExecution,
    FileBackedAgentRuntimeRegistry,
    Arc<Mutex<Vec<AgentSessionSpec>>>,
) {
    let specs = Arc::default();
    let (execution, registry) = worker_with_client(
        fixture,
        model::creation::Creations::default(),
        Box::new(Degraded {
            native: fixture.client(),
            specs: Arc::clone(&specs),
        }),
    );
    (execution, registry, specs)
}

fn owner() -> SessionOwner {
    SessionOwner {
        bot_id: BotId::generate(),
        conversation_id: ConversationId::generate(),
        workspace_id: "wks_0123456789abcdef".into(),
    }
}

fn create(id: &str, mode: &str, instructions: &str) -> Action {
    Action::Create {
        agent_id: id.into(),
        config: ExecutionConfig {
            provider: "codex".into(),
            model: None,
            mode_id: Some(mode.into()),
            thinking: None,
        },
        instructions: instructions.into(),
    }
}

fn config(registry: &FileBackedAgentRuntimeRegistry, id: &str) -> StoredAgentConfig {
    registry.get(id).unwrap().unwrap().config.unwrap()
}

fn preapproved(config: &StoredAgentConfig) -> Vec<String> {
    config.tool_policy.as_ref().unwrap()["preapproved"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            assert_eq!(entry["kind"], "mcp");
            assert_eq!(entry["server"], "ait");
            entry["tool"].as_str().unwrap().to_owned()
        })
        .collect()
}

#[tokio::test]
async fn bot_session_is_read_only_and_preapproves_grantable_ait_tools() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let owner = owner();
    let id = uuid::Uuid::new_v4().to_string();

    let created = execution
        .bot(&owner, create(&id, "full-access", "Bot instructions"))
        .await
        .unwrap();

    assert_eq!(
        created,
        json!({"created":true,"instructionDelivery":"system_prompt"})
    );
    let config = config(&registry, &id);
    assert_eq!(config.mode_id.as_deref(), Some("read-only"));
    assert_eq!(config.system_prompt.as_deref(), Some("Bot instructions"));
    let tools = preapproved(&config);
    assert!(tools.iter().any(|tool| tool == "create_agent"));
    assert!(tools.iter().any(|tool| tool == "bot_memory_update"));
    assert!(!tools.iter().any(|tool| tool == "respond_to_permission"));
    let start = fixture
        .requests()
        .into_iter()
        .find(|request| request["method"] == "thread/start")
        .unwrap();
    assert_eq!(start["params"]["sandbox"], "read-only");
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn degraded_provider_bot_session_uses_default_mode_and_turn_prefix() {
    let fixture = Fixture::new();
    let (execution, registry, specs) = degraded(&fixture);
    let owner = owner();
    let id = uuid::Uuid::new_v4().to_string();

    let created = execution
        .bot(&owner, create(&id, "bypassPermissions", "Bot instructions"))
        .await
        .unwrap();

    assert_eq!(
        created,
        json!({"created":true,"instructionDelivery":"turn_prefix"})
    );
    let spec = specs.lock().unwrap().pop().unwrap();
    assert_eq!(spec.config.mode_id, None);
    assert_eq!(spec.config.system_prompt, None);
    assert_eq!(spec.config.tool_policy, None);
    assert_eq!(config(&registry, &id).mode_id, None);
    // ADR-145: native write requests are still denied automatically.
    assert_bot_reply(&execution, &owner, &id, "permit-command", "Echo: denied").await;
    assert_bot_reply(&execution, &owner, &id, "permit-file", "Echo: denied").await;
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn existing_bot_session_is_reconfigured_to_its_profile() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let owner = owner();
    let id = uuid::Uuid::new_v4().to_string();
    execution
        .bot(&owner, create(&id, "auto", "Bot instructions"))
        .await
        .unwrap();
    execution
        .bot(
            &owner,
            Action::Release {
                agent_id: id.clone(),
            },
        )
        .await
        .unwrap();
    registry
        .update(&id, &|record| {
            let mut record = record.clone();
            let config = record.config.as_mut().unwrap();
            config.mode_id = Some("auto".into());
            config.tool_policy = None;
            record
        })
        .unwrap();

    let reused = execution
        .bot(&owner, create(&id, "auto", "Updated instructions"))
        .await
        .unwrap();

    assert_eq!(
        reused,
        json!({"created":false,"instructionDelivery":"system_prompt"})
    );
    let config = config(&registry, &id);
    assert_eq!(config.mode_id.as_deref(), Some("read-only"));
    assert_eq!(
        config.system_prompt.as_deref(),
        Some("Updated instructions")
    );
    assert!(
        preapproved(&config)
            .iter()
            .any(|tool| tool == "create_agent")
    );
    assert_bot_reply(&execution, &owner, &id, "resumed", "Echo: resumed").await;
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn legacy_degraded_bot_session_with_a_mode_requires_reset() {
    let fixture = Fixture::new();
    let (execution, registry, _) = degraded(&fixture);
    let owner = owner();
    let id = uuid::Uuid::new_v4().to_string();
    execution
        .bot(&owner, create(&id, "auto", "Bot instructions"))
        .await
        .unwrap();
    registry
        .update(&id, &|record| {
            let mut record = record.clone();
            let config = record.config.as_mut().unwrap();
            config.mode_id = Some("auto".into());
            config.system_prompt = Some("old".into());
            record
        })
        .unwrap();
    let legacy = registry.get(&id).unwrap().unwrap();

    let reused = execution
        .bot(&owner, create(&id, "auto", "Updated instructions"))
        .await
        .unwrap();

    assert_eq!(
        reused,
        json!({"created":false,"instructionDelivery":"turn_prefix","resetRequired":true})
    );
    let record = registry.get(&id).unwrap().unwrap();
    assert_eq!(record, legacy);
    assert_eq!(record.config.unwrap().system_prompt.as_deref(), Some("old"));
    execution.shutdown().await.unwrap();
}
