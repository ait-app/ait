use std::collections::BTreeSet;
use std::time::Duration;

use domain::agent_runtime::{AgentRuntimeStatus, registry::AgentRuntimeRegistry};
use model::outbound::{Frame, Outbound};
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
use serde_json::{Value, json};

use crate::local::claude::tests::fixture;
use crate::service::agent_manager::{AgentManager, AgentRegistration};
use crate::storage::timeline::Timeline;

#[tokio::test]
async fn oauth_expiry_reaches_terminal_event_snapshot_and_reopened_chat_history() {
    let (root, client, spec) = fixture();
    let registry_path = root.path().join("agents.json");
    let registry = FileBackedAgentRuntimeRegistry::new(registry_path.clone());
    let timeline_path = root.path().join("timeline.sqlite3");
    let timeline = Timeline::open(&timeline_path).unwrap();
    let mut manager = AgentManager::new(Box::new(registry.clone())).with_timeline(timeline.clone());
    manager.register_client(Box::new(client)).unwrap();
    manager
        .create("agent", &spec, AgentRegistration::default())
        .await
        .unwrap();
    let (outbound, mut output) = Outbound::new();
    let subscription = timeline.events().observe(
        "test".to_owned(),
        BTreeSet::from(["agent".to_owned()]),
        outbound,
    );
    subscription.activate().unwrap();
    manager
        .send("agent", "provider-error:assistant")
        .await
        .unwrap();
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
    assert!(message.contains("Claude Code"));
    assert!(message.contains("OAuth session expired and could not be refreshed"));
    assert!(message.contains("Sign in again"));
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
    assert_eq!(
        FileBackedAgentRuntimeRegistry::new(registry_path)
            .get("agent")
            .unwrap()
            .unwrap()
            .last_error,
        record.last_error
    );
    manager.close_all().await.unwrap();
}
