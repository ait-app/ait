use super::*;
use domain::agent_runtime::StoredAgentConfig;
use provider_core::ports::agent_session::{AgentClient, AgentSessionSpec};
use provider_core::protocol::side_questions::{SideContextEntry, SideContextSnapshot};
use std::time::Duration;

struct Fixture {
    _root: tempfile::TempDir,
    client: CodexClient,
    spec: SideRunnerSpec,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().join("work");
        std::fs::create_dir(&cwd).unwrap();
        let mut client = CodexClient::new(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/codex_side_questions.py"),
        );
        client.deadline = Duration::from_secs(2);
        Self {
            spec: SideRunnerSpec {
                environment: provider_core::ports::environment::AgentEnvironment::default(),
                directory: cwd.join("auxiliary"),
                session: AgentSessionSpec {
                    provider: "codex".into(),
                    cwd: cwd.to_str().unwrap().into(),
                    config: StoredAgentConfig {
                        model: Some("selected-model".into()),
                        ..StoredAgentConfig::default()
                    },
                    confinement: None,
                },
            },
            client,
            _root: root,
        }
    }

    fn mode(&self, mode: &str) {
        std::fs::write(PathBuf::from(&self.spec.session.cwd).join("mode"), mode).unwrap();
    }

    fn requests(&self) -> Vec<Value> {
        std::fs::read_to_string(PathBuf::from(&self.spec.session.cwd).join("requests.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    async fn runner(&self) -> Runner {
        Runner::open(self.client.clone(), self.spec.clone())
            .await
            .unwrap()
    }
}

fn ask(spec: &SideRunnerSpec, request: &str) -> SideAsk {
    SideAsk {
        request_id: request.into(),
        question: "explain the frozen context".into(),
        record_id: None,
        previous: None,
        snapshot: SideContextSnapshot {
            v: 1,
            agent_id: "agent".into(),
            provider: "codex".into(),
            model: spec.session.config.model.clone(),
            native_session_id: Some("main".into()),
            native_leaf_id: None,
            captured_at: 42,
            entries: vec![SideContextEntry {
                role: "user".into(),
                text: "frozen evidence".into(),
            }],
            truncated: true,
        },
    }
}

async fn terminal(runner: &mut dyn SideQuestions) -> SideTopic {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            runner.poll_side();
            let state = runner.side_projection();
            if let Some(topic) = state.records.first()
                && topic.latest_turn().status.is_terminal()
            {
                assert!(state.running_id.is_none());
                return topic.clone();
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}

fn reaped(pid: u32) -> bool {
    matches!(
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid.try_into().unwrap()), None),
        Err(nix::errno::Errno::ESRCH)
    )
}

#[tokio::test]
async fn capability_probe_never_submits_and_old_versions_remain_unavailable() {
    let fixture = Fixture::new();
    let mut runner = fixture
        .client
        .create_side_questions(&fixture.spec)
        .await
        .unwrap();
    assert!(runner.side_capability().available);
    assert!(
        !fixture
            .requests()
            .iter()
            .any(|request| request["method"] == "turn/start")
    );
    runner.close_side().await.unwrap();
    std::fs::write(
        PathBuf::from(&fixture.spec.session.cwd).join("version"),
        "0.166.0",
    )
    .unwrap();
    assert!(matches!(
        fixture.client.create_side_questions(&fixture.spec).await,
        Err(AgentSessionError::Unavailable)
    ));
    assert!(
        !fixture
            .requests()
            .iter()
            .any(|request| request["method"] == "turn/start")
    );
}

#[tokio::test]
async fn completed_followup_reconstructs_fixed_context_and_keeps_both_answer_messages() {
    let fixture = Fixture::new();
    let mut runner = fixture.runner().await;
    let first = ask(&fixture.spec, "first");
    runner.side_ask(first.clone()).await.unwrap();
    let topic = terminal(&mut runner).await;
    assert_eq!(topic.latest_turn().status, SideTurnStatus::Complete);
    assert_eq!(topic.latest_turn().answer, "first answer\n\nsecond answer");
    assert!(topic.truncated);
    let mut followup = ask(&fixture.spec, "followup");
    followup.record_id = Some(topic.record.id.clone());
    followup.previous = Some(topic.clone());
    followup.question = "follow up".into();
    runner.side_ask(followup).await.unwrap();
    let followed = terminal(&mut runner).await;
    assert_eq!(followed.record.answer, topic.record.answer);
    assert_eq!(
        followed.record.follow_ups[0].answer,
        "first answer\n\nsecond answer"
    );
    let requests = fixture.requests();
    let submission = requests
        .iter()
        .rev()
        .find(|request| request["method"] == "turn/start")
        .unwrap();
    let prompt: Value =
        serde_json::from_str(submission["params"]["input"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        prompt["snapshot"],
        serde_json::to_value(first.snapshot).unwrap()
    );
    assert_eq!(
        prompt["previous"],
        serde_json::to_value(topic.record).unwrap()
    );
    assert_eq!(submission["params"]["model"], "selected-model");
    assert!(reaped(
        submission["pid"].as_u64().unwrap().try_into().unwrap()
    ));
}

#[tokio::test]
async fn cancel_is_record_scoped_and_reaps_the_auxiliary_group() {
    let fixture = Fixture::new();
    fixture.mode("hold");
    let mut runner = fixture.runner().await;
    let receipt = runner.side_ask(ask(&fixture.spec, "hold")).await.unwrap();
    assert!(matches!(
        runner.side_ask(ask(&fixture.spec, "second")).await,
        Err(AgentSessionError::Busy)
    ));
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            runner.poll_side();
            if runner.topic.as_ref().unwrap().latest_turn().answer == "visible partial" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        !runner
            .side_cancel(Some("wrong-topic"))
            .await
            .unwrap()
            .cancelled
    );
    let pid = runner
        .active
        .as_ref()
        .unwrap()
        .transport
        .process_id()
        .unwrap();
    let descendant: u32 =
        std::fs::read_to_string(PathBuf::from(&fixture.spec.session.cwd).join("descendant"))
            .unwrap()
            .parse()
            .unwrap();
    assert!(
        runner
            .side_cancel(Some(&receipt.topic.record.id))
            .await
            .unwrap()
            .cancelled
    );
    assert!(reaped(pid));
    tokio::time::timeout(Duration::from_secs(3), async {
        while !reaped(descendant) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        runner.topic.as_ref().unwrap().latest_turn().status,
        SideTurnStatus::Cancelled
    );
    assert!(!locator::path(&fixture.spec.directory, "hold").exists());
}

#[tokio::test]
async fn native_tools_permissions_and_answer_overflow_fail_closed() {
    for mode in ["tool", "permission", "overflow"] {
        let fixture = Fixture::new();
        fixture.mode(mode);
        let mut runner = fixture.runner().await;
        runner.side_ask(ask(&fixture.spec, mode)).await.unwrap();
        let topic = terminal(&mut runner).await;
        assert_eq!(topic.latest_turn().status, SideTurnStatus::Error, "{mode}");
        assert!(encoded_size(&topic).unwrap() <= MAX_RECORD_BYTES);
        assert!(!locator::path(&fixture.spec.directory, mode).exists());
        let pid = fixture
            .requests()
            .iter()
            .rev()
            .find(|request| request["method"] == "turn/start")
            .unwrap()["pid"]
            .as_u64()
            .unwrap()
            .try_into()
            .unwrap();
        assert!(reaped(pid));
    }
}

#[tokio::test]
async fn lost_ack_stays_uncertain_and_never_resubmits_the_request() {
    let fixture = Fixture::new();
    fixture.mode("lost-ack");
    let mut runner = fixture.runner().await;
    let request = ask(&fixture.spec, "lost");
    assert!(matches!(
        runner.side_ask(request.clone()).await,
        Err(AgentSessionError::Failed)
    ));
    assert!(runner.reconcile_side("lost").await.unwrap().is_none());
    assert!(matches!(
        runner.side_ask(request).await,
        Err(AgentSessionError::Failed)
    ));
    assert_eq!(
        fixture
            .requests()
            .iter()
            .filter(|request| request["method"] == "turn/start")
            .count(),
        1
    );
}

#[tokio::test]
async fn recovery_never_signals_a_still_live_or_unowned_group() {
    let fixture = Fixture::new();
    fixture.mode("hold");
    let mut original = fixture.runner().await;
    original
        .side_ask(ask(&fixture.spec, "uncertain"))
        .await
        .unwrap();
    let pid = original
        .active
        .as_ref()
        .unwrap()
        .transport
        .process_id()
        .unwrap();
    let mut resumed = fixture.runner().await;
    assert!(resumed.reconcile_side("uncertain").await.is_err());
    assert!(!reaped(pid));
    original.side_cancel(None).await.unwrap();
    assert!(resumed.reconcile_side("uncertain").await.unwrap().is_none());
    assert!(
        !fixture
            .requests()
            .iter()
            .any(|request| request["method"] == "thread/resume")
    );
}

#[tokio::test]
async fn dropping_runner_reaps_its_owned_child_and_removes_locator() {
    let fixture = Fixture::new();
    fixture.mode("hold");
    let mut runner = fixture.runner().await;
    runner.side_ask(ask(&fixture.spec, "drop")).await.unwrap();
    let pid = runner
        .active
        .as_ref()
        .unwrap()
        .transport
        .process_id()
        .unwrap();
    drop(runner);
    tokio::time::timeout(Duration::from_secs(3), async {
        while !reaped(pid) || locator::path(&fixture.spec.directory, "drop").exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn changed_model_and_invalid_followup_reject_before_submission() {
    let fixture = Fixture::new();
    let mut runner = fixture.runner().await;
    let mut request = ask(&fixture.spec, "invalid");
    request.snapshot.model = Some("other-model".into());
    assert!(matches!(
        runner.side_ask(request).await,
        Err(AgentSessionError::Rejected)
    ));
    let mut request = ask(&fixture.spec, "unknown");
    request.record_id = Some("missing".into());
    assert!(matches!(
        runner.side_ask(request).await,
        Err(AgentSessionError::Rejected)
    ));
    assert!(
        !fixture
            .requests()
            .iter()
            .any(|request| request["method"] == "turn/start")
    );
}

#[tokio::test]
async fn dropped_submission_reconciles_without_claiming_acceptance_or_replaying() {
    let fixture = Fixture::new();
    fixture.mode("hold-ack");
    let mut runner = fixture.runner().await;
    let mut submission = Box::pin(runner.side_ask(ask(&fixture.spec, "abandoned")));
    let pid = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::select! {
            result = &mut submission => panic!("fixture must withhold acknowledgement: {result:?}"),
            pid = async {
                loop {
                    if let Some(request) = fixture.requests().iter().find(|request| request["method"] == "turn/start") {
                        break request["pid"].as_u64().unwrap().try_into().unwrap();
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            } => pid,
        }
    }).await.unwrap();
    drop(submission);
    assert!(runner.reconcile_side("abandoned").await.unwrap().is_none());
    assert!(reaped(pid));
    assert!(matches!(
        runner.side_ask(ask(&fixture.spec, "abandoned")).await,
        Err(AgentSessionError::Failed)
    ));
    assert_eq!(
        fixture
            .requests()
            .iter()
            .filter(|request| request["method"] == "turn/start")
            .count(),
        1
    );
}

#[tokio::test]
async fn many_follow_ups_preserve_history_and_context_budget_failure_is_explicit() {
    let fixture = Fixture::new();
    let mut runner = fixture.runner().await;
    runner.side_ask(ask(&fixture.spec, "first")).await.unwrap();
    let mut previous = terminal(&mut runner).await;
    for index in 0..9 {
        previous.record.follow_ups.push(BtwHistoryTurn {
            question: format!("earlier question {index}"),
            images: Vec::new(),
            answer: format!("earlier answer {index}"),
            status: SideTurnStatus::Complete,
            created_at: 1,
            updated_at: 2,
            error: None,
        });
    }
    let mut follow = ask(&fixture.spec, "tenth-follow-up");
    follow.record_id = Some(previous.record.id.clone());
    follow.previous = Some(previous);
    runner.side_ask(follow).await.unwrap();
    let mut previous = terminal(&mut runner).await;
    assert_eq!(previous.record.follow_ups.len(), 10);
    assert_eq!(previous.record.follow_ups[0].answer, "earlier answer 0");
    for turn in &mut previous.record.follow_ups {
        turn.answer = "x".repeat(MAX_ANSWER_BYTES);
    }
    let mut over_budget = ask(&fixture.spec, "over-budget");
    over_budget.record_id = Some(previous.record.id.clone());
    over_budget.previous = Some(previous);
    assert!(matches!(
        runner.side_ask(over_budget).await,
        Err(AgentSessionError::Rejected)
    ));
    assert_eq!(runner.side_rejection_reason(), Some("context_limit"));
    assert!(
        runner
            .side_projection()
            .error
            .unwrap()
            .contains("full session")
    );
    runner
        .side_ask(ask(&fixture.spec, "fresh-topic"))
        .await
        .unwrap();
    assert_eq!(runner.side_rejection_reason(), None);
    runner.close_side().await.unwrap();
}
