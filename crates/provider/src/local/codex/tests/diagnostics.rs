use std::time::Duration;

use domain::agent_runtime::{AgentRuntimeStatus, registry::AgentRuntimeRegistry};
use model::outbound::{Frame, Outbound};
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
use serde_json::{Value, json};

use super::terminal;
use crate::ports::agent_session::{AgentClient, AgentTurnEvent};
use crate::ports::diagnostics::Category;
use crate::service::agent_manager::{AgentManager, AgentRegistration};
use crate::storage::timeline::Timeline;
use crate::test_support::Fixture;

#[tokio::test]
async fn upstream_failures_keep_their_cause_without_resending_input() {
    for (error, expected) in [
        (
            json!({"message":"unexpected status 403 Forbidden: insufficient balance token=secret",
                "codexErrorInfo":{"httpConnectionFailed":{"httpStatusCode":403}}}),
            Category::Quota,
        ),
        (
            json!({"message":"Access blocked by Cloudflare token=secret",
                "codexErrorInfo":{"httpConnectionFailed":{"httpStatusCode":403}}}),
            Category::Permission,
        ),
        (
            json!({"message":"stream disconnected before completion: stream closed before response.completed token=secret",
                "codexErrorInfo":"other"}),
            Category::IncompleteResponse,
        ),
    ] {
        for text in ["upstream-error", "upstream-error-result"] {
            let fixture = Fixture::new();
            std::fs::write(fixture.cwd.join("native-error.json"), error.to_string()).unwrap();
            let client = fixture.client();
            let spec = fixture.spec();
            let mut session = client.create_session(&spec).await.unwrap();

            session.start_turn(text, &spec.config).await.unwrap();
            assert_eq!(
                terminal(session.as_mut()).await.unwrap(),
                AgentTurnEvent::Failed
            );
            let diagnostic = session.failure_diagnostic().unwrap();
            assert_eq!(diagnostic.category, expected);
            assert!(!format!("{diagnostic:?}").contains("secret"));
            let message = session
                .failure_message()
                .expect("native failure explanation");
            let reason = match expected {
                Category::Quota => "insufficient balance",
                Category::Permission => "Access blocked by Cloudflare",
                Category::IncompleteResponse => "stream closed before response.completed",
                _ => panic!("unexpected fixture category"),
            };
            assert!(message.contains(reason));
            assert!(!message.contains("secret"));
            if text == "upstream-error" {
                assert!(message.contains("Codex retries did not recover the request."));
            }
            assert_eq!(session.poll_turn().unwrap(), None);
            assert_eq!(
                fixture
                    .requests()
                    .iter()
                    .filter(|request| request["method"] == "turn/start")
                    .count(),
                1
            );

            session
                .start_turn("after-error", &spec.config)
                .await
                .unwrap();
            assert!(matches!(
                terminal(session.as_mut()).await.unwrap(),
                AgentTurnEvent::Completed(_)
            ));
            assert_eq!(session.failure_diagnostic(), None);
            assert_eq!(session.failure_message(), None);
            session.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn native_failure_explanation_survives_closing_its_transport() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.cwd.join("native-error.json"),
        json!({
            "message":"stream closed before response.completed token=private-value",
            "codexErrorInfo":"other"
        })
        .to_string(),
    )
    .unwrap();
    let spec = fixture.spec();
    let mut session = fixture.client().create_session(&spec).await.unwrap();
    session
        .start_turn("upstream-error-result", &spec.config)
        .await
        .unwrap();
    assert_eq!(
        terminal(session.as_mut()).await.unwrap(),
        AgentTurnEvent::Failed
    );
    let message = session.failure_message().unwrap().to_owned();

    session.close().await.unwrap();
    session.close().await.unwrap();

    assert_eq!(session.failure_message(), Some(message.as_str()));
    assert!(message.contains("stream closed before response.completed"));
    assert!(!message.contains("private-value"));
}

#[tokio::test]
async fn native_error_details_reach_chat_terminal_event_and_durable_snapshot() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.cwd.join("native-error.json"),
        json!({
            "message":"unexpected status 403 Forbidden: insufficient balance token=private-value",
            "codexErrorInfo":{"httpConnectionFailed":{"httpStatusCode":403}},
            "additionalDetails":"private diagnostic payload"
        })
        .to_string(),
    )
    .unwrap();
    let registry_path = fixture.root.path().join("agents.json");
    let registry = FileBackedAgentRuntimeRegistry::new(registry_path.clone());
    registry.initialize().unwrap();
    let timeline_path = fixture.root.path().join("timeline.sqlite3");
    let timeline = Timeline::open(&timeline_path).unwrap();
    let mut manager = AgentManager::new(Box::new(registry.clone())).with_timeline(timeline.clone());
    manager.register_client(Box::new(fixture.client())).unwrap();
    manager
        .create("agent", &fixture.spec(), AgentRegistration::default())
        .await
        .unwrap();
    let (outbound, mut output) = Outbound::new();
    let subscription = timeline.events().observe(
        "test".to_owned(),
        std::collections::BTreeSet::from(["agent".to_owned()]),
        outbound,
    );
    subscription.activate().unwrap();

    manager.send("agent", "upstream-error").await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while manager.active_turn("agent").is_some() {
            manager.poll().await.unwrap();
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let record = registry.get("agent").unwrap().unwrap();
    assert_eq!(record.last_status, AgentRuntimeStatus::Error);
    let message = record.last_error.as_ref().unwrap();
    assert!(message.contains("HTTP 403"));
    assert!(message.contains("insufficient balance"));
    assert!(message.contains("Codex retries did not recover the request."));
    assert!(!message.contains("private"));
    let mut terminal = Vec::new();
    while let Ok(frame) = output.try_recv() {
        if let Frame::Text(bytes) = frame.message {
            let event: Value = serde_json::from_str(&bytes).unwrap();
            if event["params"]["event"]["type"] == "turn_failed" {
                terminal.push(event["params"]["event"]["error"].clone());
            }
        }
    }
    assert_eq!(terminal, vec![json!(message)]);
    manager.load_timeline("agent").await.unwrap();
    let reopened = Timeline::open(&timeline_path).unwrap();
    let (_, rows) = reopened.read("agent").unwrap();
    let errors: Vec<_> = rows
        .iter()
        .filter(|row| row.entry.item["type"] == "error")
        .collect();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].entry.item["message"], *message);
    let reopened_registry = FileBackedAgentRuntimeRegistry::new(registry_path);
    assert_eq!(
        reopened_registry.get("agent").unwrap().unwrap().last_error,
        record.last_error
    );
    manager.close_all().await.unwrap();
}
