//! Real native transient-generation acceptance; model responses are loopback fixtures.

use super::*;
use provider_core::ports::side_questions::{SideAsk, SideRunnerSpec};
use provider_core::protocol::side_questions::{
    SideContextEntry, SideContextSnapshot, SideTurnStatus,
};

#[tokio::test]
#[ignore = "requires OpenCode native transient generation API; isolated loopback-only model"]
async fn installed_native_btw_keeps_fixed_context_and_never_creates_a_session() {
    let mut fixture = isolated_fixture().await;
    let spec = SideRunnerSpec {
        environment: provider_core::ports::environment::AgentEnvironment::default(),
        directory: fixture.root.path().join("aux"),
        session: fixture.spec.clone(),
    };
    let mut runner = fixture.client.create_side_questions(&spec).await.unwrap();
    assert!(
        fixture.requests.try_recv().is_err(),
        "capability probe must never call a model"
    );
    let snapshot = SideContextSnapshot {
        v: 1,
        agent_id: "source".into(),
        provider: "opencode".into(),
        model: fixture.spec.config.model.clone(),
        captured_at: 1,
        native_session_id: Some("main-native-identity".into()),
        native_leaf_id: None,
        entries: vec![SideContextEntry {
            role: "user".into(),
            text: "fixed source context".into(),
        }],
        truncated: false,
    };
    let mut previous = None;
    for index in 0..2 {
        let record_id = previous.as_ref().map(
            |topic: &provider_core::protocol::side_questions::SideTopic| topic.record.id.clone(),
        );
        runner
            .side_ask(SideAsk {
                request_id: format!("request-{index}"),
                question: format!("frozen side question native test {index}"),
                record_id,
                snapshot: snapshot.clone(),
                previous: previous.clone(),
            })
            .await
            .unwrap();
        let request = tokio::time::timeout(Duration::from_secs(10), fixture.requests.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(request["tools"].as_array().is_none_or(Vec::is_empty));
        assert!(
            request["messages"]
                .to_string()
                .contains("fixed source context")
        );
        if index == 1 {
            assert!(
                request["messages"]
                    .to_string()
                    .contains("authoritative answer")
            );
        }
        previous = Some(
            tokio::time::timeout(Duration::from_secs(10), async {
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
            .unwrap(),
        );
        assert_eq!(
            previous.as_ref().unwrap().latest_turn().status,
            SideTurnStatus::Complete
        );
        assert_eq!(
            previous.as_ref().unwrap().latest_turn().answer,
            "authoritative answer"
        );
    }
    runner.close_side().await.unwrap();
    let sessions = fixture
        .client
        .list_sessions(&provider_core::ports::native_history::ListOptions {
            cwd: Some(fixture.spec.cwd.clone()),
            scan_limit: 100,
        })
        .await
        .unwrap();
    assert!(
        sessions.is_empty(),
        "BTW must not create native working sessions"
    );
    assert_eq!(std::fs::read_dir(spec.directory).unwrap().count(), 0);
}

/// Native nonstreaming text generation uses the same isolated loopback-only model fixture.
pub(super) fn model_response(request: &Value) -> Option<Response> {
    if !request["messages"]
        .to_string()
        .contains("frozen side question native test")
    {
        return None;
    }
    assert!(request["tools"].as_array().is_none_or(Vec::is_empty));
    (request["stream"] != true).then(|| Json(json!({"id":"btw-native-fixture","object":"chat.completion","created":0,"model":"test-model",
        "choices":[{"index":0,"message":{"role":"assistant","content":"authoritative answer"},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}})).into_response())
}

async fn isolated_fixture() -> Installed {
    let mut fixture = installed(false).await;
    let home = fixture.root.path().join("home");
    let workspace = fixture.root.path().join("work");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::rename(
        fixture.root.path().join("opencode.json"),
        workspace.join("opencode.json"),
    )
    .unwrap();
    fixture
        .client
        .environment
        .insert("HOME".into(), home.to_string_lossy().into_owned());
    fixture.client.environment.insert(
        "OPENCODE_TEST_HOME".into(),
        home.to_string_lossy().into_owned(),
    );
    fixture.spec.cwd = workspace
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();

    fixture
}
