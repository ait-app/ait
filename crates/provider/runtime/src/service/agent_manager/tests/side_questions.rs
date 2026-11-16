mod deadline;

use super::*;
use crate::storage::side::SideStore;
use crate::storage::timeline::Timeline;
use provider_core::ports::side_questions::{
    SideAsk, SideAskReceipt, SideCancelOutcome, SideCapability, SideQuestions,
};
use provider_core::protocol::side_questions::{
    BtwHistoryRecord, SideAskRequest, SideCancelRequest, SideHistoryRequest, SideQuestionsState,
    SideReceiptState, SideStrategy, SideTopic, SideTurnStatus,
};

#[derive(Debug, Default)]
struct RunnerState {
    asks: usize,
    lose_ack: bool,
    fail_cancel: bool,
    reject_context: bool,
    topic: Option<SideTopic>,
    snapshots: Vec<provider_core::protocol::side_questions::SideContextSnapshot>,
}

#[derive(Debug, Clone, Default)]
struct Runner(Arc<Mutex<RunnerState>>);

#[async_trait::async_trait]
impl SideQuestions for Runner {
    fn side_strategy(&self) -> SideStrategy {
        SideStrategy::Isolated
    }
    fn side_capability(&self) -> SideCapability {
        SideCapability::available()
    }

    fn side_rejection_reason(&self) -> Option<&str> {
        self.0
            .lock()
            .expect("runner")
            .reject_context
            .then_some(provider_core::protocol::side_questions::reason::CONTEXT_LIMIT)
    }

    async fn side_ask(&mut self, ask: SideAsk) -> Result<SideAskReceipt, AgentSessionError> {
        let mut state = self.0.lock().unwrap();
        if state.reject_context {
            return Err(AgentSessionError::Rejected);
        }
        state.asks += 1;
        state.snapshots.push(ask.snapshot.clone());
        let topic = if let Some(mut topic) = ask.previous {
            let timestamp = u64::try_from(state.asks).expect("bounded fixture count") * 2;
            topic
                .record
                .follow_ups
                .push(provider_core::protocol::side_questions::BtwHistoryTurn {
                    question: ask.question,
                    images: Vec::new(),
                    answer: String::new(),
                    status: SideTurnStatus::Running,
                    created_at: timestamp,
                    updated_at: timestamp,
                    error: None,
                });
            topic
        } else {
            SideTopic {
                record: BtwHistoryRecord {
                    id: format!("topic-{}", state.asks),
                    question: ask.question.clone(),
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
            }
        };
        state.topic = Some(topic.clone());
        if state.lose_ack {
            return Err(AgentSessionError::Failed);
        }
        Ok(SideAskReceipt {
            strategy: topic.strategy,
            topic,
        })
    }

    async fn side_cancel(
        &mut self,
        _: Option<&str>,
    ) -> Result<SideCancelOutcome, AgentSessionError> {
        let mut state = self.0.lock().unwrap();
        if state.fail_cancel {
            return Err(AgentSessionError::Failed);
        }
        let cancelled = state.topic.as_mut().is_some_and(|topic| {
            if topic.latest_turn().status != SideTurnStatus::Running {
                return false;
            }
            if let Some(turn) = topic.record.follow_ups.last_mut() {
                turn.status = SideTurnStatus::Cancelled;
                turn.updated_at += 1;
            } else {
                topic.record.status = SideTurnStatus::Cancelled;
                topic.record.updated_at += 1;
            }
            true
        });
        Ok(SideCancelOutcome { cancelled })
    }

    async fn side_history(&mut self) -> Result<Vec<SideTopic>, AgentSessionError> {
        Ok(self.0.lock().unwrap().topic.iter().cloned().collect())
    }

    async fn reconcile_side(&mut self, _: &str) -> Result<Option<SideTopic>, AgentSessionError> {
        let state = self.0.lock().unwrap();
        if state.lose_ack {
            return Err(AgentSessionError::Failed);
        }
        Ok(state.topic.clone())
    }

    fn side_projection(&self) -> SideQuestionsState {
        let state = self.0.lock().unwrap();
        SideQuestionsState {
            error: state.reject_context.then(|| {
                "BTW context limit reached; expand this side conversation to continue.".to_owned()
            }),
            records: state.topic.iter().cloned().collect(),
            running_id: state
                .topic
                .as_ref()
                .filter(|topic| topic.latest_turn().status == SideTurnStatus::Running)
                .map(|topic| topic.record.id.clone()),
            ..Default::default()
        }
    }

    fn poll_side(&mut self) -> Option<SideQuestionsState> {
        Some(self.side_projection())
    }
}

struct Fixture {
    manager: AgentManager,
    registry: MemoryRegistry,
    main: FakeClient,
    runner: Runner,
    _directory: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let (mut manager, registry, main) = make_manager();
    registry.upsert(&stored_record()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    manager = manager
        .with_timeline(Timeline::memory().unwrap())
        .with_side_store(SideStore::open(&directory.path().join("side.sqlite3")).unwrap());
    let runner = Runner::default();
    manager
        .side_runners
        .insert("agent-1".to_owned(), Box::new(runner.clone()));
    Fixture {
        manager,
        registry,
        main,
        runner,
        _directory: directory,
    }
}

fn ask(request: &str) -> SideAskRequest {
    SideAskRequest {
        agent_id: "agent-1".to_owned(),
        request_id: request.to_owned(),
        question: "explain the committed result".to_owned(),
        record_id: None,
    }
}

fn history() -> SideHistoryRequest {
    SideHistoryRequest {
        agent_id: "agent-1".to_owned(),
    }
}

#[tokio::test]
async fn replay_and_busy_admission_leave_main_state_unchanged() {
    let mut f = fixture();
    let before = f.registry.get("agent-1").unwrap();
    let first = f.manager.side_ask(ask("first")).await.unwrap();
    assert_eq!(first.receipt, SideReceiptState::Accepted);
    let replay = f.manager.side_ask(ask("first")).await.unwrap();
    assert_eq!(replay.record_id, first.record_id);
    let busy = f.manager.side_ask(ask("second")).await.unwrap();
    assert_eq!(busy.receipt, SideReceiptState::Rejected);
    assert_eq!(busy.reason.as_deref(), Some("busy"));
    assert_eq!(f.runner.0.lock().unwrap().asks, 1);
    assert_eq!(f.registry.get("agent-1").unwrap(), before);
    let main = f.main.0.lock().unwrap();
    assert_eq!(main.create_calls, 0);
    assert_eq!(main.start_calls, 0);
    assert!(main.resume_specs.is_empty());
}

#[tokio::test]
async fn lost_ack_blocks_new_execution_until_authoritative_reconciliation() {
    let mut f = fixture();
    f.runner.0.lock().unwrap().lose_ack = true;
    assert_eq!(
        f.manager.side_ask(ask("first")).await.unwrap().receipt,
        SideReceiptState::Uncertain
    );
    assert_eq!(
        f.manager.side_ask(ask("first")).await.unwrap().receipt,
        SideReceiptState::Uncertain
    );
    assert_eq!(
        f.manager.side_ask(ask("second")).await.unwrap().receipt,
        SideReceiptState::Rejected
    );
    f.runner.0.lock().unwrap().lose_ack = false;
    let recovered = f.manager.side_history(history()).await.unwrap();
    assert_eq!(recovered.running_id.as_deref(), Some("topic-1"));
    assert_eq!(
        f.manager.side_ask(ask("first")).await.unwrap().receipt,
        SideReceiptState::Accepted
    );
    assert_eq!(f.runner.0.lock().unwrap().asks, 1);
}

#[tokio::test]
async fn failed_cancellation_keeps_the_running_topic_and_success_is_durable() {
    let mut f = fixture();
    f.manager.side_ask(ask("first")).await.unwrap();
    let cancel = || SideCancelRequest {
        agent_id: "agent-1".to_owned(),
        record_id: None,
    };
    f.runner.0.lock().unwrap().fail_cancel = true;
    assert!(matches!(
        f.manager.side_cancel(cancel()).await,
        Err(crate::rpc::ErrorCode::AgentIo)
    ));
    assert_eq!(
        f.manager
            .side_projection("agent-1")
            .unwrap()
            .running_id
            .as_deref(),
        Some("topic-1")
    );
    f.runner.0.lock().unwrap().fail_cancel = false;
    assert!(f.manager.side_cancel(cancel()).await.unwrap().cancelled);
    assert_eq!(
        f.manager.side_history(history()).await.unwrap().records[0]
            .latest_turn()
            .status,
        SideTurnStatus::Cancelled
    );
    assert_eq!(f.main.0.lock().unwrap().start_calls, 0);
}

#[tokio::test]
async fn unavailable_runtime_keeps_cached_history_readable_without_starting_main() {
    let mut f = fixture();
    f.manager.side_ask(ask("first")).await.unwrap();
    f.manager.side_runners.clear();
    let cached = f.manager.side_history(history()).await.unwrap();
    assert_eq!(
        cached.records[0].latest_turn().status,
        SideTurnStatus::Interrupted
    );
    assert_eq!(
        cached.records[0].record.question,
        "explain the committed result"
    );
    assert!(cached.running_id.is_none());
    assert_eq!(f.main.0.lock().unwrap().create_calls, 0);
    assert!(f.main.0.lock().unwrap().resume_specs.is_empty());
}

#[tokio::test]
async fn transfer_preserves_every_followup_without_creating_a_writer_or_running_inference() {
    let mut f = fixture();
    f.manager
        .side_ask(ask("first"))
        .await
        .expect("accepted side question");
    let mut topic = f
        .runner
        .0
        .lock()
        .expect("runner")
        .topic
        .clone()
        .expect("topic");
    topic.record.status = SideTurnStatus::Complete;
    topic.record.answer = "original answer".to_owned();
    for index in 0..12 {
        topic
            .record
            .follow_ups
            .push(provider_core::protocol::side_questions::BtwHistoryTurn {
                question: format!("followup {index}"),
                images: Vec::new(),
                answer: format!("response {index}"),
                status: SideTurnStatus::Complete,
                created_at: index + 10,
                updated_at: index + 11,
                error: None,
            });
    }
    f.runner.0.lock().expect("runner").topic = Some(topic.clone());
    let before = f.registry.get("agent-1").expect("source record");
    let bundle = f
        .manager
        .side_transfer(
            &provider_core::protocol::side_questions::SideTransferRequest {
                agent_id: "agent-1".to_owned(),
                record_id: "topic-1".to_owned(),
            },
        )
        .expect("full transfer");
    assert_eq!(bundle.topic, topic);
    assert_eq!(bundle.topic.record.turn_count(), 13);
    assert_eq!(bundle.snapshot.agent_id, "agent-1");
    assert_eq!(bundle.snapshot.model.as_deref(), Some("gpt-6"));
    assert_eq!(f.runner.0.lock().expect("runner").asks, 1);
    assert_eq!(f.main.0.lock().expect("main").start_calls, 0);
    assert_eq!(f.main.0.lock().expect("main").create_calls, 0);
    assert_eq!(f.registry.get("agent-1").expect("source record"), before);
}

#[tokio::test]
async fn transfer_rejects_a_running_or_unknown_topic() {
    let mut f = fixture();
    f.manager
        .side_ask(ask("first"))
        .await
        .expect("side question");
    let request = |id: &str| provider_core::protocol::side_questions::SideTransferRequest {
        agent_id: "agent-1".to_owned(),
        record_id: id.to_owned(),
    };
    assert!(matches!(
        f.manager.side_transfer(&request("topic-1")),
        Err(crate::rpc::ErrorCode::CatalogBusy)
    ));
    assert!(matches!(
        f.manager.side_transfer(&request("missing")),
        Err(crate::rpc::ErrorCode::InvalidMessage)
    ));
}

#[derive(Debug)]
struct Factory {
    runner: Runner,
    specs: Arc<Mutex<Vec<provider_core::ports::side_questions::SideRunnerSpec>>>,
}

#[async_trait::async_trait]
impl AgentClient for Factory {
    fn provider(&self) -> &'static str {
        "future-native-provider"
    }
    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        Ok(true)
    }
    async fn create_session(
        &self,
        _: &AgentSessionSpec,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        Err(AgentSessionError::Failed)
    }
    async fn resume_session(
        &self,
        _: &AgentPersistenceHandle,
        _: &AgentSessionSpec,
        _: AgentResumePurpose,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        Err(AgentSessionError::Failed)
    }
    async fn create_side_questions(
        &self,
        spec: &provider_core::ports::side_questions::SideRunnerSpec,
    ) -> Result<Box<dyn SideQuestions>, AgentSessionError> {
        self.specs
            .lock()
            .expect("factory selections")
            .push(spec.clone());
        if spec.session.config.model.is_none() {
            return Err(AgentSessionError::Unavailable);
        }
        Ok(Box::new(self.runner.clone()))
    }
}

fn finish(runner: &Runner) {
    let mut state = runner.0.lock().expect("runner");
    let topic = state.topic.as_mut().expect("accepted topic");
    if let Some(turn) = topic.record.follow_ups.last_mut() {
        turn.status = SideTurnStatus::Complete;
        turn.answer = "followup answer".to_owned();
        turn.updated_at += 1;
    } else {
        topic.record.status = SideTurnStatus::Complete;
        topic.record.answer = "root answer".to_owned();
        topic.record.updated_at += 1;
    }
}

#[tokio::test]
async fn future_adapter_uses_selected_model_and_rebuilds_for_new_and_original_topic_selections() {
    let (mut f, specs) = future_fixture(Some("selected-a"));
    f.manager
        .side_history(history())
        .await
        .expect("no-inference capability probe");
    f.manager
        .side_ask(ask("first"))
        .await
        .expect("first question");
    f.registry
        .update("agent-1", &|record| {
            let mut record = record.clone();
            record.config.as_mut().expect("config").thinking_option_id = Some("high".to_owned());
            record
        })
        .expect("change main settings while BTW runs");
    assert_eq!(
        f.manager
            .side_history(history())
            .await
            .expect("active side survives main configuration changes")
            .running_id
            .as_deref(),
        Some("topic-1")
    );
    finish(&f.runner);
    f.manager.sync_side_topics("agent-1").expect("first result");
    f.registry
        .update("agent-1", &|record| {
            let mut record = record.clone();
            record.config.as_mut().expect("config").model = Some("selected-b".to_owned());
            record
        })
        .expect("change selection");
    select_fixture_environment(&f.manager);
    f.manager
        .side_ask(ask("second"))
        .await
        .expect("new topic selection");
    finish(&f.runner);
    f.manager
        .sync_side_topics("agent-1")
        .expect("second result");
    let followup = SideAskRequest {
        record_id: Some("topic-1".to_owned()),
        ..ask("followup")
    };
    f.manager
        .side_ask(followup)
        .await
        .expect("original topic selection");
    finish(&f.runner);
    f.manager
        .sync_side_topics("agent-1")
        .expect("followup result");
    age_side_runner(&mut f.manager);
    f.manager
        .poll_side_runners()
        .await
        .expect("reclaim idle runner");
    assert!(!f.manager.side_runners.contains_key("agent-1"));
    f.manager
        .side_ask(SideAskRequest {
            record_id: Some("topic-1".to_owned()),
            ..ask("after-idle")
        })
        .await
        .expect("followup after idle reclamation");
    let selected = specs.lock().expect("factory selections");
    let models: Vec<_> = selected
        .iter()
        .map(|spec| spec.session.config.model.as_deref())
        .collect();
    assert_eq!(
        models,
        [
            Some("selected-a"),
            Some("selected-b"),
            Some("selected-a"),
            Some("selected-a")
        ]
    );
    assert_eq!(
        selected[1]
            .environment
            .variable("AIT_TEST_SIDE_ENV")
            .as_deref(),
        Some("fixture-selection")
    );
    assert_eq!(
        f.runner.0.lock().expect("runner").snapshots[2]
            .model
            .as_deref(),
        Some("selected-a")
    );
    assert_eq!(f.main.0.lock().expect("main").create_calls, 0);
}

#[tokio::test]
async fn side_capacity_rejects_before_execution_and_idle_runner_cleanup_preserves_history() {
    let mut f = fixture();
    let budget = f.manager.side_budget.clone();
    let held: Vec<_> = (0..8)
        .map(|_| {
            budget
                .clone()
                .try_acquire_owned()
                .expect("available side slot")
        })
        .collect();
    let rejected = f
        .manager
        .side_ask(ask("over-capacity"))
        .await
        .expect("capacity rejection");
    assert_eq!(rejected.reason.as_deref(), Some("capacity"));
    assert_eq!(f.runner.0.lock().expect("runner").asks, 0);
    drop(held);
    f.manager
        .side_ask(ask("first"))
        .await
        .expect("side admission");
    age_side_runner(&mut f.manager);
    f.manager
        .poll_side_runners()
        .await
        .expect("active side is retained");
    assert!(f.manager.side_runners.contains_key("agent-1"));
    finish(&f.runner);
    f.manager
        .sync_side_topics("agent-1")
        .expect("persist final answer");
    f.manager
        .poll_side_runners()
        .await
        .expect("idle side reaped");
    assert!(!f.manager.side_runners.contains_key("agent-1"));
    assert_eq!(
        f.manager
            .side_projection("agent-1")
            .expect("retained history")
            .records[0]
            .record
            .answer,
        "root answer"
    );
}

#[tokio::test]
async fn history_without_a_selected_model_never_locks_the_later_ask_to_that_probe() {
    let (mut f, specs) = future_fixture(None);
    assert_eq!(
        f.manager
            .side_history(history())
            .await
            .expect("read-only unavailable history")
            .available,
        Some(false)
    );
    assert_eq!(
        f.manager
            .side_ask(ask("no-model"))
            .await
            .expect("pre-send rejection")
            .reason
            .as_deref(),
        Some("no_selected_model")
    );
    assert_eq!(f.runner.0.lock().expect("runner").asks, 0);
    f.registry
        .update("agent-1", &|record| {
            let mut record = record.clone();
            record.config.as_mut().expect("config").model = Some("selected-now".to_owned());
            record
        })
        .expect("select model");
    assert_eq!(
        f.manager
            .side_ask(ask("selected-model"))
            .await
            .expect("new runner")
            .receipt,
        SideReceiptState::Accepted
    );
    assert_eq!(
        specs
            .lock()
            .expect("probe selections")
            .last()
            .expect("selected runner")
            .session
            .config
            .model
            .as_deref(),
        Some("selected-now")
    );
}

fn future_fixture(
    model: Option<&str>,
) -> (
    Fixture,
    Arc<Mutex<Vec<provider_core::ports::side_questions::SideRunnerSpec>>>,
) {
    let mut f = fixture();
    f.manager.side_runners.clear();
    let specs = Arc::new(Mutex::new(Vec::new()));
    f.manager
        .register_client(Box::new(Factory {
            runner: f.runner.clone(),
            specs: specs.clone(),
        }))
        .expect("register future adapter");
    f.registry
        .update("agent-1", &|record| {
            let mut record = record.clone();
            record.provider = "future-native-provider".to_owned();
            record.config.as_mut().expect("config").model = model.map(str::to_owned);
            record.runtime_info = None;
            record
        })
        .expect("select provider");
    (f, specs)
}

fn age_side_runner(manager: &mut AgentManager) {
    manager.side_activity.insert(
        "agent-1".to_owned(),
        std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(61))
            .expect("host monotonic clock has an idle interval"),
    );
}

fn select_fixture_environment(manager: &AgentManager) {
    manager
        .launch_environments
        .lock()
        .expect("launch environments")
        .insert(
            "agent-1".to_owned(),
            provider_core::ports::environment::AgentEnvironment::try_from(BTreeMap::from([(
                "AIT_TEST_SIDE_ENV".to_owned(),
                "fixture-selection".to_owned(),
            )]))
            .expect("fixture environment"),
        );
}

#[tokio::test]
async fn provider_context_rejection_keeps_its_specific_reason_and_panel_error_without_inference() {
    let mut f = fixture();
    f.runner.0.lock().expect("runner").reject_context = true;
    let response = f
        .manager
        .side_ask(ask("context-limit"))
        .await
        .expect("provider rejection");
    assert_eq!(response.receipt, SideReceiptState::Rejected);
    assert_eq!(
        response.reason.as_deref(),
        Some(provider_core::protocol::side_questions::reason::CONTEXT_LIMIT)
    );
    assert!(
        f.manager
            .side_projection("agent-1")
            .expect("panel state")
            .error
            .expect("specific error")
            .contains("context limit")
    );
    assert_eq!(f.runner.0.lock().expect("runner").asks, 0);
    assert_eq!(f.main.0.lock().expect("main").start_calls, 0);
}

#[tokio::test]
async fn expiring_an_idle_btw_runner_never_closes_or_interrupts_the_running_foreground_writer() {
    let mut f = fixture();
    f.manager.mark_history_loaded("agent-1");
    f.manager
        .send("agent-1", "continue foreground work")
        .await
        .expect("main turn");
    let turn = f
        .manager
        .active_turn("agent-1")
        .expect("active foreground turn")
        .to_owned();
    f.manager
        .side_ask(ask("side-question"))
        .await
        .expect("parallel side question");
    finish(&f.runner);
    f.manager
        .sync_side_topics("agent-1")
        .expect("side completion");
    age_side_runner(&mut f.manager);
    f.manager
        .poll_side_runners()
        .await
        .expect("reclaim auxiliary metadata");
    assert!(!f.manager.side_runners.contains_key("agent-1"));
    assert_eq!(f.manager.active_turn("agent-1"), Some(turn.as_str()));
    assert_eq!(f.main.0.lock().expect("main writer").close_calls, 0);
    f.main.0.lock().expect("main writer").events.push_back(AgentTurnEvent::Timeline(
        provider_core::protocol::timeline::NativeItem {
            key: "foreground-after-side-reclaim".to_owned(), turn_id: Some(turn.clone()),
            timestamp: "2026-11-13T00:00:00Z".to_owned(),
            item: serde_json::json!({"type":"assistant_message","text":"foreground still streaming"}),
        }));
    f.manager
        .poll()
        .await
        .expect("foreground events still drain");
    assert_eq!(f.manager.active_turn("agent-1"), Some(turn.as_str()));
    assert!(
        f.manager
            .timeline
            .as_ref()
            .expect("timeline")
            .read("agent-1")
            .expect("persisted foreground output")
            .1
            .iter()
            .any(|row| row.entry.item["text"] == "foreground still streaming")
    );
    assert_eq!(f.main.0.lock().expect("main writer").close_calls, 0);
}
