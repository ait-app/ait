//! Deterministic child-control adapter for cross-crate transport tests.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use provider_core::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionSpec,
    AgentTurnEvent,
};
use serde_json::{Value, json};

use crate::service::agent_manager::{AgentManager, AgentManagerError};

/// Shared state and adapter registration for the child-control fixture.
#[derive(Clone, Debug, Default)]
pub struct Fixture(Arc<State>);

impl Fixture {
    /// Register the deterministic child-control adapter with `manager`.
    ///
    /// # Errors
    /// Returns the manager error if the adapter cannot be registered.
    pub fn register(&self, manager: &mut AgentManager) -> Result<(), AgentManagerError> {
        manager.register_client(Box::new(Client(self.0.clone())))
    }

    /// Wait until a child control is admitted.
    pub async fn control_admitted(&self) {
        self.0.control_admitted.notified().await;
    }

    /// Return the number of admitted child controls.
    #[must_use]
    pub fn controls(&self) -> usize {
        self.0.controls.load(Ordering::SeqCst)
    }

    /// Return the number of answered permission requests.
    #[must_use]
    pub fn answers(&self) -> usize {
        self.0.answers.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Default)]
struct State {
    controls: AtomicUsize,
    control_admitted: tokio::sync::Notify,
    answers: AtomicUsize,
    pending: AtomicBool,
    events: Mutex<VecDeque<AgentTurnEvent>>,
    child: Mutex<Option<provider_core::ports::controls::NativeSubagent>>,
}

#[derive(Clone, Debug)]
struct Client(Arc<State>);

#[async_trait::async_trait]
impl AgentClient for Client {
    fn provider(&self) -> &'static str {
        "test-child-controls"
    }
    fn validate_config(&self, _: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        Ok(())
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
}

#[derive(Debug)]
struct Session {
    state: Arc<State>,
    cwd: String,
}

#[async_trait::async_trait]
impl AgentSession for Session {
    fn provider(&self) -> &'static str {
        "test-child-controls"
    }
    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        Some(AgentPersistenceHandle {
            provider: self.provider().to_owned(),
            session_id: "native-root".to_owned(),
            native_handle: None,
            metadata: None,
        })
    }
    async fn runtime_info(&mut self) -> Result<StoredAgentRuntimeInfo, AgentSessionError> {
        Ok(StoredAgentRuntimeInfo {
            provider: self.provider().to_owned(),
            session_id: Some("native-root".to_owned()),
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
        let child = provider_core::ports::controls::NativeSubagent {
            id: "native-child".to_owned(),
            parent_id: "native-root".to_owned(),
            cwd: self.cwd.clone(),
            persistence: None,
            descriptor: json!({"id":"native-child", "status":"running", "updatedAt":"2026-11-23T00:00:00Z"}),
        };
        *self
            .state
            .child
            .lock()
            .expect("fixture mutex is not poisoned") = Some(child.clone());
        self.state.pending.store(true, Ordering::SeqCst);
        let mut events = self
            .state
            .events
            .lock()
            .expect("fixture mutex is not poisoned");
        events.push_back(AgentTurnEvent::Subagent(
            provider_core::ports::controls::SubagentEvent::Upsert(child),
        ));
        events.push_back(AgentTurnEvent::PermissionRequested(
            json!({"id":"permission-1", "toolName":"fixture"}),
        ));
        Ok("turn-1".to_owned())
    }
    fn subagents(&self) -> Vec<provider_core::ports::controls::NativeSubagent> {
        self.state
            .child
            .lock()
            .expect("fixture mutex is not poisoned")
            .iter()
            .cloned()
            .collect()
    }
    fn pending_permissions(&self) -> Vec<Value> {
        if self.state.pending.load(Ordering::SeqCst) {
            vec![json!({"id":"permission-1", "toolName":"fixture"})]
        } else {
            Vec::new()
        }
    }
    fn begin_child_guide(
        &mut self,
        _: &str,
        _: &str,
    ) -> Result<provider_core::ports::controls::ChildControlReceipt, AgentSessionError> {
        self.state.controls.fetch_add(1, Ordering::SeqCst);
        self.state.control_admitted.notify_one();
        Ok(provider_core::ports::controls::ChildControlReceipt {
            token: "receipt".to_owned(),
        })
    }
    async fn respond_permission(&mut self, id: &str, _: &Value) -> Result<(), AgentSessionError> {
        if id != "permission-1" || !self.state.pending.swap(false, Ordering::SeqCst) {
            return Err(AgentSessionError::Rejected);
        }
        self.state.answers.fetch_add(1, Ordering::SeqCst);
        self.state
            .events
            .lock()
            .expect("fixture mutex is not poisoned")
            .push_back(AgentTurnEvent::ChildControl {
                token: "receipt".to_owned(),
                outcome: provider_core::ports::controls::ChildControlOutcome::Guide(Ok(
                    provider_core::ports::controls::ChildGuideOutcome::Accepted,
                )),
            });
        Ok(())
    }
    fn poll_turn(&mut self) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        Ok(self
            .state
            .events
            .lock()
            .expect("fixture mutex is not poisoned")
            .pop_front())
    }
    async fn close(&mut self) -> Result<(), AgentSessionError> {
        Ok(())
    }
}
