//! A deterministic session for runtime permission-projection tests.

use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};

use crate::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionSpec,
    AgentTurnEvent,
};

#[derive(Debug)]
pub(super) struct Client {
    root: PathBuf,
    pending_on_create: bool,
}

impl Client {
    pub(super) fn new(root: PathBuf, pending_on_create: bool) -> Self {
        Self {
            root,
            pending_on_create,
        }
    }
}

fn request(secret: bool) -> Value {
    json!({"id":"dialog-1", "provider":"fixture-secret", "kind":"question",
        "input":{"questions":[{"header":"Live question","answerKey":"value","secret":secret}]},
        "actions":[{"id":"allow","label":"Submit","behavior":"allow"},
                   {"id":"deny","label":"Cancel","behavior":"deny"}]})
}

#[async_trait::async_trait]
impl AgentClient for Client {
    fn provider(&self) -> &'static str {
        "fixture-secret"
    }
    fn validate_config(&self, _: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        Ok(())
    }
    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        Ok(true)
    }
    async fn create_session(
        &self,
        _: &AgentSessionSpec,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        Ok(Box::new(Session {
            root: self.root.clone(),
            pending: self.pending_on_create.then(|| request(true)),
            events: VecDeque::new(),
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
}

#[derive(Debug)]
struct Session {
    root: PathBuf,
    pending: Option<Value>,
    events: VecDeque<AgentTurnEvent>,
}

#[async_trait::async_trait]
impl AgentSession for Session {
    fn provider(&self) -> &'static str {
        "fixture-secret"
    }
    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        None
    }
    async fn runtime_info(&mut self) -> Result<StoredAgentRuntimeInfo, AgentSessionError> {
        Ok(StoredAgentRuntimeInfo {
            provider: self.provider().to_owned(),
            session_id: None,
            model: None,
            thinking_option_id: None,
            mode_id: None,
            extra: None,
        })
    }
    async fn start_turn(
        &mut self,
        _: &str,
        _: &StoredAgentConfig,
    ) -> Result<String, AgentSessionError> {
        let bytes =
            std::fs::read(self.root.join("dialog.json")).map_err(|_| AgentSessionError::Failed)?;
        let dialog: Value =
            serde_json::from_slice(&bytes).map_err(|_| AgentSessionError::Failed)?;
        let pending = request(dialog["secret"] == true);
        self.pending = Some(pending.clone());
        self.events
            .push_back(AgentTurnEvent::PermissionRequested(pending));
        Ok("turn-1".to_owned())
    }
    fn pending_permissions(&self) -> Vec<Value> {
        self.pending.iter().cloned().collect()
    }
    async fn respond_permission(
        &mut self,
        id: &str,
        response: &Value,
    ) -> Result<(), AgentSessionError> {
        if id != "dialog-1" || self.pending.is_none() {
            return Err(AgentSessionError::Rejected);
        }
        let native = if response["behavior"] == "allow" {
            json!({"type":"extension_ui_response","id":id,"value":response["updatedInput"]["answers"]["value"]})
        } else {
            json!({"type":"extension_ui_response","id":id,"cancelled":true})
        };
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.root.join("calls.jsonl"))
            .map_err(|_| AgentSessionError::Failed)?;
        writeln!(file, "{native}").map_err(|_| AgentSessionError::Failed)?;
        self.pending.take();
        self.events.push_back(AgentTurnEvent::Completed(None));
        Ok(())
    }
    fn poll_turn(&mut self) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        Ok(self.events.pop_front())
    }
    async fn close(&mut self) -> Result<(), AgentSessionError> {
        Ok(())
    }
}
