use std::sync::{Arc, Mutex};

use domain::agent_runtime::{AgentRuntimeStatus, registry::AgentRuntimeRegistry};
use domain::session::protocol::EventsRequest;
use model::outbound::{Frame, Outbound};
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

use super::*;
use crate::local::antigravity::diagnostics::Failure;
use crate::service::agent_manager::{AgentManager, AgentRegistration};
use crate::storage::timeline::Timeline;

#[tokio::test]
async fn persists_failure_details_in_tools_snapshots_terminal_events_and_activity() {
    for (scenario, expected) in [
        ("denied", Failure::Permission),
        ("denied-unfinished", Failure::Permission),
        ("stderr-failure", Failure::Quota),
        ("stderr-flood", Failure::Quota),
        ("turn-malformed", Failure::Protocol),
    ] {
        assert_failure(scenario, expected).await;
    }
}

async fn assert_failure(scenario: &str, expected: Failure) {
    let (directory, mut client, spec) = fixture();
    client.environment = AgentEnvironment::try_from(BTreeMap::from([(
        "AGY_FIXTURE_SCENARIO".to_owned(),
        scenario.to_owned(),
    )]))
    .unwrap();
    let registry = FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json"));
    let timeline = Timeline::memory().unwrap();
    let mut manager = AgentManager::new(Box::new(registry.clone())).with_timeline(timeline.clone());
    let environment = client.environment.clone();
    manager.register_client(Box::new(client)).unwrap();
    let activity = Arc::new(Mutex::new(Vec::new()));
    let observer = activity.clone();
    let connection = manager.events().connect();
    let subscription = connection
        .subscribe(
            EventsRequest {
                events: vec!["activity_log".to_owned()],
                notifications: false,
            },
            Arc::new(move |_, payload| {
                observer.lock().unwrap().push(payload);
                Ok(())
            }),
        )
        .unwrap();
    subscription.activate().unwrap();
    let (outbound, mut output) = Outbound::new();
    let stream = timeline.events().observe(
        "test".to_owned(),
        std::collections::BTreeSet::from(["agy-agent".to_owned()]),
        outbound,
    );
    stream.activate().unwrap();
    manager
        .create_with_environment(
            "agy-agent",
            &spec,
            AgentRegistration::default(),
            &environment,
        )
        .await
        .unwrap();
    manager.send("agy-agent", "diagnostic").await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while manager.active_turn("agy-agent").is_some() {
            manager.poll().await.unwrap();
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let record = registry.get("agy-agent").unwrap().unwrap();
    assert_eq!(record.last_status, AgentRuntimeStatus::Error, "{scenario}");
    assert_eq!(
        record.last_error.as_deref(),
        Some(expected.message()),
        "{scenario}"
    );
    assert!(
        activity
            .lock()
            .unwrap()
            .iter()
            .any(|event| event["content"] == expected.message())
    );
    let (_, rows) = timeline.read("agy-agent").unwrap();
    let tools: Vec<_> = rows
        .iter()
        .filter(|row| row.entry.item["type"] == "tool_call")
        .collect();
    let tool = tools.last().unwrap();
    assert!(tools.iter().all(|row| row.entry.key == tool.entry.key));
    assert_eq!(tool.entry.item["status"], "failed", "{scenario}");
    assert_eq!(
        tool.entry.item["error"]["message"],
        expected.message(),
        "{scenario}"
    );
    assert!(
        !serde_json::to_string(&record)
            .unwrap()
            .contains("fixture-secret")
    );
    let mut terminal = None;
    while let Ok(frame) = output.try_recv() {
        if let Frame::Text(bytes) = frame.message {
            let message: Value = serde_json::from_str(&bytes).unwrap();
            if message["params"]["event"]["type"] == "turn_failed" {
                terminal = Some(message["params"]["event"]["error"].clone());
            }
        }
    }
    assert_eq!(terminal, Some(json!(expected.message())), "{scenario}");
}

#[tokio::test]
async fn preserves_successful_responses_and_does_not_fail_empty_successful_tools() {
    for (scenario, status, response) in [
        ("denied-response", "failed", "Command was denied"),
        ("empty-tool", "completed", "Tool completed"),
    ] {
        let (_directory, mut client, spec) = fixture();
        client.environment = AgentEnvironment::try_from(BTreeMap::from([(
            "AGY_FIXTURE_SCENARIO".to_owned(),
            scenario.to_owned(),
        )]))
        .unwrap();
        let mut session = client.create_session(&spec).await.unwrap();
        session
            .start_turn("diagnostic", &spec.config)
            .await
            .unwrap();
        let events = completed(session.as_mut()).await;
        assert!(
            events
                .iter()
                .any(|event| matches!(event, AgentTurnEvent::Timeline(entry)
            if entry.item["type"] == "tool_call" && entry.item["status"] == status))
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, AgentTurnEvent::Timeline(entry)
            if entry.item["text"] == response))
        );
        session.close().await.unwrap();
        assert_eq!(session.failure_message(), None);
    }
}
