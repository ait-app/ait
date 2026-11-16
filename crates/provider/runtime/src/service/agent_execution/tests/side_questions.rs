use super::*;
use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use provider_core::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionSpec,
};
use provider_core::ports::side_questions::{
    SideAsk, SideAskReceipt, SideCancelOutcome, SideCapability, SideQuestions, SideRunnerSpec,
};
use provider_core::protocol::side_questions::{
    BtwHistoryRecord, SideQuestionsState, SideStrategy, SideTopic, SideTurnStatus,
};
use tokio::sync::{Notify, Semaphore};

#[derive(Debug)]
struct Gate {
    entered: Notify,
    release: Semaphore,
    topic: Mutex<Option<SideTopic>>,
}

#[derive(Debug)]
struct Client {
    native: provider_codex::CodexClient,
    gate: Arc<Gate>,
}

#[async_trait::async_trait]
impl AgentClient for Client {
    fn provider(&self) -> &str {
        self.native.provider()
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
    async fn create_session(
        &self,
        spec: &AgentSessionSpec,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
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
    async fn create_side_questions(
        &self,
        _spec: &SideRunnerSpec,
    ) -> Result<Box<dyn SideQuestions>, AgentSessionError> {
        Ok(Box::new(Runner(self.gate.clone())))
    }
}

#[derive(Debug)]
struct Runner(Arc<Gate>);

#[async_trait::async_trait]
impl SideQuestions for Runner {
    fn side_strategy(&self) -> SideStrategy {
        SideStrategy::Isolated
    }
    fn side_capability(&self) -> SideCapability {
        SideCapability::available()
    }
    async fn side_ask(&mut self, ask: SideAsk) -> Result<SideAskReceipt, AgentSessionError> {
        self.0.entered.notify_one();
        self.0
            .release
            .acquire()
            .await
            .expect("release auxiliary admission")
            .forget();
        let topic = SideTopic {
            record: BtwHistoryRecord {
                id: "side-topic".to_owned(),
                question: ask.question,
                images: Vec::new(),
                answer: String::new(),
                status: SideTurnStatus::Running,
                created_at: 1,
                updated_at: 1,
                error: None,
                leaf_id: None,
                title: None,
                follow_ups: Vec::new(),
            },
            strategy: SideStrategy::Isolated,
            truncated: false,
        };
        *self.0.topic.lock().expect("side topic") = Some(topic.clone());
        Ok(SideAskReceipt {
            strategy: topic.strategy,
            topic,
        })
    }
    async fn side_cancel(
        &mut self,
        _record: Option<&str>,
    ) -> Result<SideCancelOutcome, AgentSessionError> {
        if let Some(topic) = self.0.topic.lock().expect("side topic").as_mut() {
            topic.record.status = SideTurnStatus::Cancelled;
            topic.record.updated_at = 2;
        }
        Ok(SideCancelOutcome { cancelled: true })
    }
    async fn side_history(&mut self) -> Result<Vec<SideTopic>, AgentSessionError> {
        Ok(self
            .0
            .topic
            .lock()
            .expect("side topic")
            .iter()
            .cloned()
            .collect())
    }
    async fn reconcile_side(
        &mut self,
        _request: &str,
    ) -> Result<Option<SideTopic>, AgentSessionError> {
        Ok(self.0.topic.lock().expect("side topic").clone())
    }
    fn side_projection(&self) -> SideQuestionsState {
        let topic = self.0.topic.lock().expect("side topic");
        SideQuestionsState {
            records: topic.iter().cloned().collect(),
            running_id: topic
                .as_ref()
                .filter(|topic| topic.record.status == SideTurnStatus::Running)
                .map(|topic| topic.record.id.clone()),
            ..SideQuestionsState::default()
        }
    }
    fn poll_side(&mut self) -> Option<SideQuestionsState> {
        Some(self.side_projection())
    }
}

#[tokio::test]
async fn blocked_side_admission_does_not_block_main_cancel_progress_or_next_prompt() {
    let fixture = Fixture::new();
    let gate = Arc::new(Gate {
        entered: Notify::new(),
        release: Semaphore::new(0),
        topic: Mutex::new(None),
    });
    let execution = side_worker(&fixture, &gate);
    let created = execution
        .execute(
            "agent.create.request",
            json!({"config":{
                "provider":"codex", "cwd":fixture.cwd, "model":"gpt-6"
            }}),
        )
        .await
        .expect("main session");
    let id = created["agentId"].as_str().expect("agent id").to_owned();
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"hang"}),
        )
        .await
        .expect("main running");
    let side = {
        let execution = execution.clone();
        let id = id.clone();
        tokio::spawn(async move {
            execution
                .execute(
                    "agent.side_question.ask.request",
                    json!({"agentId":id,"requestId":"side-first","question":"why?"}),
                )
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(5), gate.entered.notified())
        .await
        .expect("side admission started");
    assert!(!side.is_finished());
    assert_side_lifecycle_scoped(&execution, &id);
    execute_ready(
        &execution,
        "agent.cancel.request",
        json!({"agentId":id}),
        "main cancellation remains independent",
    )
    .await;
    execute_ready(
        &execution,
        "agent.message.send.request",
        json!({"agentId":id,"text":"continues while side waits"}),
        "main input does not wait for side admission",
    )
    .await;
    let finished = execute_ready(
        &execution,
        "agent.finish.wait.request",
        json!({"agentId":id}),
        "main progress continues while side admission is blocked",
    )
    .await;
    assert_eq!(finished["lastMessage"], "Echo: continues while side waits");
    assert!(!side.is_finished());
    gate.release.add_permits(1);
    assert_eq!(
        side.await.expect("side task").expect("side accepted")["receipt"],
        "accepted"
    );
    let read = execution
        .execute("agent.get.request", json!({"agentId":id}))
        .await
        .expect("main directory projection");
    assert_eq!(
        read["agent"]["providerUi"]["sideQuestions"]["runningId"],
        "side-topic"
    );
    assert_eq!(
        read["agent"]["providerUi"]["sideQuestions"]["records"][0]["status"],
        "running"
    );
    assert_eq!(
        execution
            .execute("agent.side_question.cancel.request", json!({"agentId":id}))
            .await
            .expect("side cancellation")["cancelled"],
        true
    );
    let history = execution
        .execute("agent.side_question.history.request", json!({"agentId":id}))
        .await
        .expect("side history");
    assert_eq!(history["records"][0]["status"], "cancelled");
    execution.shutdown().await.expect("worker shutdown");
}

async fn execute_ready(
    execution: &AgentExecution,
    method: &str,
    params: Value,
    reason: &str,
) -> Value {
    tokio::time::timeout(Duration::from_secs(5), execution.execute(method, params))
        .await
        .expect(reason)
        .expect("runtime operation")
}

fn side_worker(fixture: &Fixture, gate: &Arc<Gate>) -> AgentExecution {
    let side_store =
        crate::storage::side::SideStore::open(&fixture.root.path().join("side.sqlite3"))
            .expect("side store");
    let (execution, _) = crate::testing::spawn(
        fixture.root.path(),
        &fixture.cwd,
        model::creation::Creations::default(),
        Box::new(Client {
            native: fixture.client(),
            gate: gate.clone(),
        }),
        |manager| manager.with_side_store(side_store),
    );
    execution
}

fn assert_side_lifecycle_scoped(execution: &AgentExecution, id: &str) {
    let related = execution
        .0
        .template
        .lock()
        .expect("worker state")
        .owners
        .related(&std::collections::BTreeSet::from([format!("agent:{id}")]))
        .expect("stable lifecycle scopes");
    assert!(related.contains(&format!("side:{id}")));
}
