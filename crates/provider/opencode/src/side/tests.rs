use super::*;
use domain::agent_runtime::StoredAgentConfig;
use provider_core::ports::agent_session::{AgentClient, AgentSessionSpec};
use provider_core::protocol::side_questions::{SideContextEntry, SideContextSnapshot};
use serde_json::Value;
use std::{path::Path, time::Duration};

struct Fixture {
    root: tempfile::TempDir,
    client: OpenCodeClient,
    spec: SideRunnerSpec,
}

impl Fixture {
    fn new(mode: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("mode"), mode).unwrap();
        let mut client = OpenCodeClient::new(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/fixtures/side.py"),
        );
        client.deadline = Duration::from_secs(2);
        client.startup_timeout = Duration::from_secs(3);
        let spec = SideRunnerSpec {
            environment: provider_core::ports::environment::AgentEnvironment::default(),
            directory: root.path().join("aux"),
            session: AgentSessionSpec {
                provider: "opencode".into(),
                cwd: root.path().canonicalize().unwrap().to_str().unwrap().into(),
                config: StoredAgentConfig {
                    model: Some("test/model".into()),
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
                v: 1,
                agent_id: "source".into(),
                provider: "opencode".into(),
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
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let records = runner.side_history().await.unwrap();
            if let Some(topic) = records
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
async fn native_transient_generation_reuses_fixed_context_and_never_opens_a_session() {
    let fixture = Fixture::new("");
    let mut runner = fixture
        .client
        .create_side_questions(&fixture.spec)
        .await
        .unwrap();
    assert!(
        fixture
            .requests()
            .iter()
            .all(|request| request["method"] == "GET")
    );
    assert!(!format!("{runner:?}").contains("private-fixture-model-key"));
    assert!(!format!("{runner:?}").contains("private-latest-header"));
    runner.side_ask(fixture.ask("first")).await.unwrap();
    let previous = settled(runner.as_mut()).await;
    assert_eq!(previous.record.answer, "frozen answer");
    let mut follow = fixture.ask("follow");
    follow.record_id = Some(previous.record.id.clone());
    follow.previous = Some(previous);
    runner.side_ask(follow).await.unwrap();
    settled(runner.as_mut()).await;
    let requests = fixture.requests();
    let asks: Vec<_> = requests
        .iter()
        .filter(|request| request["method"] == "POST")
        .collect();
    assert_eq!(asks.len(), 2);
    assert!(
        asks.iter()
            .all(|request| request["path"] == "/api/experimental/generate"
                && request["authenticated"] == true)
    );
    assert_eq!(
        asks[0]["payload"]["model"],
        json!({"providerID":"test","id":"model"})
    );
    assert!(
        asks[1]["payload"]["prompt"]
            .as_str()
            .unwrap()
            .contains("fixed source context")
    );
    assert!(
        asks[1]["payload"]["prompt"]
            .as_str()
            .unwrap()
            .contains("frozen answer")
    );
    assert_eq!(
        std::fs::read_dir(&fixture.spec.directory).unwrap().count(),
        0
    );
}

#[tokio::test]
async fn cancellation_reaps_the_owned_server_and_unsupported_hosts_never_generate() {
    let fixture = Fixture::new("delay");
    let mut runner = fixture
        .client
        .create_side_questions(&fixture.spec)
        .await
        .unwrap();
    let first = runner.side_ask(fixture.ask("first")).await.unwrap();
    assert!(matches!(
        runner.side_ask(fixture.ask("busy")).await,
        Err(AgentSessionError::Busy)
    ));
    assert!(
        !runner
            .side_cancel(Some("wrong-topic"))
            .await
            .unwrap()
            .cancelled
    );
    assert!(
        runner
            .side_cancel(Some(&first.topic.record.id))
            .await
            .unwrap()
            .cancelled
    );
    assert_eq!(
        std::fs::read_dir(&fixture.spec.directory).unwrap().count(),
        0
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
        unsupported
            .requests()
            .iter()
            .all(|request| request["method"] == "GET")
    );
}
