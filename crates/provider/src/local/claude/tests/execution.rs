//! Claude discovery, execution, rewind and durable restart contracts.

use serde_json::{Value, json};

use super::super::ClaudeClient;
use crate::service::agent_execution::AgentExecution;
use crate::test_support::{Fixture, request, worker_with_client};

mod diagnostics;
mod rewind;

fn worker(fixture: &Fixture) -> AgentExecution {
    let program =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude_code.py");
    let mut client = ClaudeClient::new(program);
    client.config_dir = Some(fixture.root.path().join("claude-config"));
    worker_with_client(
        fixture,
        model::creation::Creations::default(),
        Box::new(client),
    )
    .0
}

#[tokio::test]
async fn discovery_execution_and_restore_preserve_native_history() {
    let fixture = Fixture::new();
    let execution = worker(&fixture);
    let models = request(
        &execution,
        "provider.models.list.request",
        json!({"provider":"claude","cwd":fixture.cwd}),
    )
    .await;
    assert_eq!(models["models"][0]["id"], "sonnet", "{models}");
    let snapshot = request(
        &execution,
        "provider.snapshot.get.request",
        json!({"cwd":fixture.cwd}),
    )
    .await;
    assert!(
        snapshot["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["provider"] == "claude" && entry["status"] == "ready")
    );
    let created = request(
        &execution,
        "agent.create.request",
        json!({"config":{
            "provider":"claude","cwd":fixture.cwd,"model":"sonnet","modeId":"default"
        }}),
    )
    .await;
    let id = created["agentId"]
        .as_str()
        .unwrap_or_else(|| panic!("{created}"))
        .to_owned();
    assert_eq!(created["agent"]["persistence"]["provider"], "claude");
    let sent = request(
        &execution,
        "agent.message.send.request",
        json!({"agentId":id,"text":"first"}),
    )
    .await;
    assert_eq!(sent["accepted"], true, "{sent}");
    let finished = request(
        &execution,
        "agent.finish.wait.request",
        json!({"agentId":id}),
    )
    .await;
    assert_eq!(finished["lastMessage"], "Claude: first", "{finished}");
    let timeline = request(
        &execution,
        "agent.timeline.get.request",
        json!({"agentId":id}),
    )
    .await;
    assert!(timeline.to_string().contains("Claude: first"), "{timeline}");
    execution.shutdown().await.unwrap();
    let execution = worker(&fixture);
    let resumed = request(
        &execution,
        "agent.resume.request",
        json!({"handle":created["agent"]["persistence"]}),
    )
    .await;
    assert_eq!(resumed["agentId"], id, "{resumed}");
    request(
        &execution,
        "agent.message.send.request",
        json!({"agentId":id,"text":"second"}),
    )
    .await;
    let finished = request(
        &execution,
        "agent.finish.wait.request",
        json!({"agentId":id}),
    )
    .await;
    assert_eq!(finished["lastMessage"], "Claude: second", "{finished}");
    let timeline = request(
        &execution,
        "agent.timeline.get.request",
        json!({"agentId":id}),
    )
    .await;
    assert!(
        timeline.to_string().contains("Claude: second"),
        "{timeline}"
    );
    execution.shutdown().await.unwrap();
}
