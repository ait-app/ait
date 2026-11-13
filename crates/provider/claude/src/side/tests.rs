use super::*;
use domain::agent_runtime::StoredAgentConfig;
use provider_core::ports::agent_session::{AgentClient, AgentSessionSpec};
use provider_core::protocol::side_questions::{SideContextEntry, SideContextSnapshot};
use std::path::Path;

struct Fixture {
    root: tempfile::TempDir,
    client: ClaudeClient,
    spec: SideRunnerSpec,
}

impl Fixture {
    fn new(mode: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("mode"), mode).unwrap();
        let mut client = ClaudeClient::new(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude_side_questions.py"),
        );
        client.deadline = Duration::from_secs(2);
        let spec = SideRunnerSpec {
            environment: provider_core::ports::environment::AgentEnvironment::default(),
            directory: root.path().join("aux"),
            session: AgentSessionSpec {
                provider: "claude".into(),
                cwd: root.path().to_str().unwrap().into(),
                config: StoredAgentConfig {
                    model: Some("selected-model".into()),
                    ..StoredAgentConfig::default()
                },
                confinement: None,
            },
        };
        Self { root, client, spec }
    }

    fn ask(&self, id: &str) -> SideAsk {
        SideAsk {
            request_id: id.into(),
            question: "why this design?".into(),
            record_id: None,
            previous: None,
            snapshot: SideContextSnapshot {
                agent_id: "source-agent".into(),
                v: 1,
                provider: "claude".into(),
                model: self.spec.session.config.model.clone(),
                captured_at: 1,
                native_session_id: None,
                native_leaf_id: None,
                entries: vec![SideContextEntry {
                    role: "user".into(),
                    text: "fixed source context".into(),
                }],
                truncated: false,
            },
        }
    }
}

async fn settled(runner: &mut dyn SideQuestions) -> SideTopic {
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let history = runner.side_history().await.unwrap();
            if let Some(topic) = history
                .first()
                .filter(|topic| topic.latest_turn().status.is_terminal())
            {
                return topic.clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn isolated_same_model_stream_and_follow_up_never_use_main_session() {
    let fixture = Fixture::new("");
    let mut runner = fixture
        .client
        .create_side_questions(&fixture.spec)
        .await
        .unwrap();
    assert!(!fixture.root.path().join("calls.jsonl").exists());
    runner.side_ask(fixture.ask("first")).await.unwrap();
    let previous = settled(runner.as_mut()).await;
    assert_eq!(previous.record.answer, "frozen answer");
    let mut follow = fixture.ask("second");
    follow.record_id = Some(previous.record.id.clone());
    follow.previous = Some(previous);
    follow.question = "continue".into();
    runner.side_ask(follow).await.unwrap();
    let completed = settled(runner.as_mut()).await;
    assert_eq!(completed.record.follow_ups[0].answer, "frozen answer");
    let calls: Vec<Value> = std::fs::read_to_string(fixture.root.path().join("calls.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(calls.len(), 2);
    assert!(
        calls[1]["prompt"]
            .as_str()
            .unwrap()
            .contains("fixed source context")
    );
    assert!(
        calls[1]["prompt"]
            .as_str()
            .unwrap()
            .contains("frozen answer")
    );
    let args = calls[0]["args"].as_array().unwrap();
    for flag in [
        "--tools=",
        "--strict-mcp-config",
        "--safe-mode",
        "--setting-sources=",
        "--no-session-persistence",
    ] {
        assert!(args.iter().any(|arg| arg == flag));
    }
    assert!(
        !args
            .iter()
            .any(|arg| arg == "--resume" || arg == "--fallback-model")
    );
    assert_eq!(
        std::fs::read_dir(&fixture.spec.directory).unwrap().count(),
        0
    );
}

#[tokio::test]
async fn cancel_is_scoped_and_confirmed_after_auxiliary_exit() {
    let fixture = Fixture::new("delay");
    let mut runner = fixture
        .client
        .create_side_questions(&fixture.spec)
        .await
        .unwrap();
    let accepted = runner.side_ask(fixture.ask("first")).await.unwrap();
    assert!(matches!(
        runner.side_ask(fixture.ask("second")).await,
        Err(AgentSessionError::Busy)
    ));
    assert!(
        !runner
            .side_cancel(Some("other-topic"))
            .await
            .unwrap()
            .cancelled
    );
    assert!(
        runner
            .side_cancel(Some(&accepted.topic.record.id))
            .await
            .unwrap()
            .cancelled
    );
    assert_eq!(
        runner.side_history().await.unwrap()[0].latest_turn().status,
        SideTurnStatus::Cancelled
    );
    assert_eq!(
        std::fs::read_dir(&fixture.spec.directory).unwrap().count(),
        0
    );
}

#[tokio::test]
async fn unexpected_tools_or_native_error_fail_closed_without_leaking_credentials() {
    let fixture = Fixture::new("tools");
    let mut runner = fixture
        .client
        .create_side_questions(&fixture.spec)
        .await
        .unwrap();
    assert!(runner.side_ask(fixture.ask("first")).await.is_err());
    for mode in ["tool-call", "error"] {
        let fixture = Fixture::new(mode);
        let mut runner = fixture
            .client
            .create_side_questions(&fixture.spec)
            .await
            .unwrap();
        runner.side_ask(fixture.ask("first")).await.unwrap();
        let topic = settled(runner.as_mut()).await;
        assert_eq!(topic.latest_turn().status, SideTurnStatus::Error);
        assert!(
            !serde_json::to_string(&topic)
                .unwrap()
                .contains("secret-fixture-only")
        );
        assert_eq!(
            std::fs::read_dir(&fixture.spec.directory).unwrap().count(),
            0
        );
    }
}

#[tokio::test]
async fn restarted_runner_cannot_release_a_still_live_auxiliary_request() {
    let fixture = Fixture::new("delay");
    let mut original = fixture
        .client
        .create_side_questions(&fixture.spec)
        .await
        .unwrap();
    original.side_ask(fixture.ask("recover")).await.unwrap();
    let mut recovered = fixture
        .client
        .create_side_questions(&fixture.spec)
        .await
        .unwrap();
    assert!(recovered.reconcile_side("recover").await.is_err());
    assert_eq!(
        original.side_history().await.unwrap()[0]
            .latest_turn()
            .status,
        SideTurnStatus::Running
    );
    original.side_cancel(None).await.unwrap();
    assert!(recovered.reconcile_side("recover").await.unwrap().is_none());
}
