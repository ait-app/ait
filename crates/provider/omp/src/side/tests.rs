use super::*;
use domain::agent_runtime::StoredAgentConfig;
use provider_core::ports::agent_session::{AgentClient, AgentSessionSpec};
use provider_core::protocol::side_questions::{SideContextEntry, SideContextSnapshot};
use std::path::Path;
use std::time::Duration;

struct Fixture {
    root: tempfile::TempDir,
    client: OmpClient,
    spec: SideRunnerSpec,
}

impl Fixture {
    fn new(mode: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("mode"), mode).unwrap();
        let mut client = OmpClient::new(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/omp_side_questions.py"),
        );
        client.timeouts = super::super::transport::Timeouts {
            ready: Duration::from_secs(2),
            request: Duration::from_secs(2),
        };
        let spec = SideRunnerSpec {
            environment: provider_core::ports::environment::AgentEnvironment::default(),
            session: AgentSessionSpec {
                provider: "omp".into(),
                cwd: root.path().to_str().unwrap().into(),
                config: StoredAgentConfig {
                    model: Some(r#"["test","model"]"#.into()),
                    ..StoredAgentConfig::default()
                },
                confinement: None,
            },
            directory: root.path().join("aux"),
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
                provider: "omp".into(),
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

    fn requests(&self) -> Vec<Value> {
        std::fs::read_to_string(self.root.path().join("requests.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

async fn settled(runner: &mut dyn SideQuestions) -> SideTopic {
    tokio::time::timeout(Duration::from_secs(3), async {
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
async fn native_btw_streams_and_follow_up_reuses_only_frozen_context() {
    let fixture = Fixture::new("");
    let mut runner = fixture
        .client
        .create_side_questions(&fixture.spec)
        .await
        .unwrap();
    assert!(
        !fixture
            .requests()
            .iter()
            .any(|request| request["type"] == "btw")
    );
    let receipt = runner.side_ask(fixture.ask("first")).await.unwrap();
    assert_eq!(receipt.strategy, SideStrategy::Native);
    assert_eq!(receipt.topic.record.question, "why this design?");
    let previous = settled(runner.as_mut()).await;
    assert_eq!(previous.record.answer, "frozen answer");
    let mut follow = fixture.ask("second");
    follow.record_id = Some(previous.record.id.clone());
    follow.previous = Some(previous);
    follow.question = "explain further".into();
    runner.side_ask(follow).await.unwrap();
    let result = settled(runner.as_mut()).await;
    assert_eq!(result.record.follow_ups.len(), 1);
    assert_eq!(result.record.follow_ups[0].answer, "frozen answer");
    let requests = fixture.requests();
    assert!(
        requests
            .iter()
            .all(|request| matches!(request["type"].as_str(), Some("get_btw_history" | "btw")))
    );
    let asks: Vec<_> = requests
        .iter()
        .filter(|request| request["type"] == "btw")
        .collect();
    assert!(
        asks[1]["question"]
            .as_str()
            .unwrap()
            .contains("fixed source context")
    );
    assert!(
        asks[1]["question"]
            .as_str()
            .unwrap()
            .contains("frozen answer")
    );
    for args in std::fs::read_to_string(fixture.root.path().join("args.jsonl"))
        .unwrap()
        .lines()
    {
        let args: Vec<String> = serde_json::from_str(args).unwrap();
        for flag in [
            "--no-extensions",
            "--no-skills",
            "--no-rules",
            "--no-context-files",
            "--no-session",
        ] {
            assert!(args.iter().any(|arg| arg == flag));
        }
        assert!(!args.iter().any(|arg| arg == "--no-tools"));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--model", "test/model"])
        );
    }
    runner.close_side().await.unwrap();
}

#[tokio::test]
async fn native_busy_cancel_and_unsupported_do_not_fall_back() {
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
            .side_cancel(Some("different-topic"))
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
    assert!(
        fixture
            .requests()
            .iter()
            .all(|request| request["type"] != "prompt")
    );
    let unsupported = Fixture::new("unsupported");
    assert!(
        unsupported
            .client
            .create_side_questions(&unsupported.spec)
            .await
            .is_err()
    );
    assert!(
        !unsupported
            .requests()
            .iter()
            .any(|request| request["type"] == "btw")
    );
}

#[tokio::test]
async fn recovered_auxiliary_ownership_stays_uncertain_until_the_original_group_exits() {
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
