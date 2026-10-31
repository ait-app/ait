use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};

use crate::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionSpec,
    AgentTurnEvent,
};
use crate::ports::controls::{NativeSubagent, SubagentEvent};
use crate::ports::native_history::{SessionDescriptor, SessionHistory};
use crate::protocol::prompt::AgentPrompt;
use crate::protocol::timeline::NativeItem;
use crate::service::agent_execution::AgentExecution;
use crate::test_support::{Fixture, request, worker_with_client};

const NOW: &str = "2026-10-31T00:00:00Z";

#[derive(Debug, Default)]
pub(super) struct State {
    pub(super) cancellations: usize,
    pub(super) closes: usize,
    pub(super) history_reads: usize,
    events: VecDeque<AgentTurnEvent>,
    child: Option<NativeSubagent>,
}

pub(super) struct Harness {
    pub(super) execution: AgentExecution,
    pub(super) state: Arc<Mutex<State>>,
    pub(super) id: String,
    pub(super) created: Value,
    _files: Fixture,
}

impl Harness {
    pub(super) async fn new() -> Self {
        let files = Fixture::new();
        let state = Arc::new(Mutex::new(State::default()));
        let (execution, _) = worker_with_client(
            &files,
            model::creation::Creations::default(),
            Box::new(Client(state.clone())),
        );
        let created = request(
            &execution,
            "agent.create.request",
            json!({"config":{
                "provider":"contract", "cwd":files.cwd, "title":"Contract session"
            }}),
        )
        .await;
        Self {
            id: created["agentId"].as_str().unwrap().to_owned(),
            created,
            execution,
            state,
            _files: files,
        }
    }

    pub(super) async fn send(&self) {
        let sent = request(
            &self.execution,
            "agent.message.send.request",
            json!({
                "agentId":self.id, "text":"keep running"
            }),
        )
        .await;
        assert_eq!(sent["accepted"], true);
    }

    pub(super) async fn snapshot(&self) -> Value {
        request(
            &self.execution,
            "agent.get.request",
            json!({"agentId":self.id}),
        )
        .await
    }
}

#[derive(Debug)]
struct Client(Arc<Mutex<State>>);

#[async_trait::async_trait]
impl AgentClient for Client {
    fn provider(&self) -> &'static str {
        "contract"
    }

    fn supports_history_replay(&self) -> bool {
        false
    }

    fn validate_config(&self, _: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        Ok(())
    }

    fn settings(&self, _: &StoredAgentConfig) -> Value {
        json!({"availableModes":[], "features":[], "capabilities":{
            "supportsRewindConversation":false, "supportsRewindFiles":false,
            "supportsRewindBoth":false
        }})
    }

    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        Ok(true)
    }

    async fn create_session(
        &self,
        spec: &AgentSessionSpec,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        Ok(Box::new(Session {
            state: self.0.clone(),
            cwd: spec.cwd.clone(),
        }))
    }

    async fn resume_session(
        &self,
        _: &AgentPersistenceHandle,
        spec: &AgentSessionSpec,
        _: AgentResumePurpose,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        self.create_session(spec).await
    }

    async fn inspect_session(
        &self,
        handle: &AgentPersistenceHandle,
        cwd: &str,
    ) -> Result<SessionHistory, AgentSessionError> {
        self.0.lock().unwrap().history_reads += 1;
        Ok(SessionHistory {
            resume_metadata: BTreeMap::new(),
            parent_id: Some("native-root".to_owned()),
            descriptor: SessionDescriptor {
                provider_id: "contract".to_owned(),
                provider_label: "Contract".to_owned(),
                provider_handle_id: handle.session_id.clone(),
                cwd: cwd.to_owned(),
                title: None,
                first_prompt_preview: None,
                last_prompt_preview: None,
                last_activity_at: NOW.to_owned(),
            },
            created_at: NOW.to_owned(),
            config: StoredAgentConfig::default(),
            active: true,
            // The native file intentionally lags behind the child's live output.
            entries: Vec::new(),
        })
    }
}

#[derive(Debug)]
struct Session {
    state: Arc<Mutex<State>>,
    cwd: String,
}

#[async_trait::async_trait]
impl AgentSession for Session {
    fn provider(&self) -> &'static str {
        "contract"
    }

    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        Some(AgentPersistenceHandle {
            provider: "contract".to_owned(),
            session_id: "native-root".to_owned(),
            native_handle: None,
            metadata: None,
        })
    }

    async fn runtime_info(&mut self) -> Result<StoredAgentRuntimeInfo, AgentSessionError> {
        Ok(StoredAgentRuntimeInfo {
            provider: "contract".to_owned(),
            session_id: Some("native-root".to_owned()),
            model: None,
            thinking_option_id: None,
            mode_id: None,
            extra: None,
        })
    }

    fn control_settings(&self, config: &StoredAgentConfig) -> Option<Value> {
        Some(
            json!({"availableModes":[{"id":"native", "label":"Native mode"}],
            "features":[{"id":"fastMode", "value":config.model.as_deref() == Some("fast")}]}),
        )
    }

    async fn start_input(
        &mut self,
        prompt: &AgentPrompt,
        _: &StoredAgentConfig,
    ) -> Result<String, AgentSessionError> {
        prompt.validate()?;
        let child = NativeSubagent {
            persistence: None,
            id: "native-child".to_owned(),
            parent_id: "native-root".to_owned(),
            cwd: self.cwd.clone(),
            descriptor: json!({"id":"native-child", "status":"running", "updatedAt":NOW}),
        };
        let mut state = self.state.lock().unwrap();
        state.child = Some(child.clone());
        state
            .events
            .push_back(AgentTurnEvent::Subagent(SubagentEvent::Upsert(child)));
        state.events.push_back(AgentTurnEvent::Subagent(SubagentEvent::Progress {
            id:"native-child".to_owned(), observation:"child-progress".to_owned(),
            entry:NativeItem { key:"native:child:answer".to_owned(), turn_id:Some("child-turn".to_owned()), timestamp:NOW.to_owned(),
                item:json!({"type":"assistant_message", "messageId":"child-answer", "text":"Live child output"}) },
        }));
        Ok("native-turn".to_owned())
    }

    fn subagents(&self) -> Vec<NativeSubagent> {
        self.state.lock().unwrap().child.iter().cloned().collect()
    }

    fn poll_turn(&mut self) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        Ok(self.state.lock().unwrap().events.pop_front())
    }

    async fn cancel_turn(&mut self, _: &str) -> Result<(), AgentSessionError> {
        let mut state = self.state.lock().unwrap();
        state.cancellations += 1;
        state.events.push_back(AgentTurnEvent::Cancelled);
        Ok(())
    }

    async fn close(&mut self) -> Result<(), AgentSessionError> {
        self.state.lock().unwrap().closes += 1;
        Ok(())
    }
}
