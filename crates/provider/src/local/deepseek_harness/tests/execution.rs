//! ACP permissions, timeline persistence and worker restart recovery.

use std::time::Duration;

use serde_json::json;

use super::super::DeepSeekHarnessClient;
use crate::service::agent_execution::AgentExecution;
use crate::test_support::{Fixture, request, worker_with_client};

fn worker(fixture: &Fixture) -> AgentExecution {
    let program = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/local/deepseek_harness/tests/fixtures/acp.cjs");
    let client = DeepSeekHarnessClient::new(program).with_acp_profile();
    worker_with_client(
        fixture,
        model::creation::Creations::default(),
        Box::new(client),
    )
    .0
}

#[tokio::test]
async fn acp_executes_and_keeps_history_after_worker_restart() {
    let fixture = Fixture::new();
    let cwd = &fixture.cwd;
    let execution = worker(&fixture);
    request(
        &execution,
        "provider.models.list.request",
        json!({"provider":"deepseek-harness","cwd":cwd}),
    )
    .await;
    let snapshot = request(
        &execution,
        "provider.snapshot.get.request",
        json!({"cwd":cwd}),
    )
    .await;
    let dsh = snapshot["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["provider"] == "deepseek-harness")
        .unwrap();
    assert_eq!(dsh["label"], "DeepSeek Harness");
    assert_eq!(dsh["status"], "ready");
    let created = request(
        &execution,
        "agent.create.request",
        json!({"config":{
        "provider":"deepseek-harness","cwd":cwd,"title":"ACP process test"}}),
    )
    .await;
    let id = created["agentId"].as_str().unwrap();
    let handle = &created["agent"]["persistence"];
    approve_and_finish(&execution, id).await;
    let before = request(
        &execution,
        "agent.timeline.get.request",
        json!({"agentId":id,"limit":0}),
    )
    .await;
    assert!(
        before["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["item"]["text"] == "Hello world")
    );
    execution.shutdown().await.unwrap();

    let execution = worker(&fixture);
    let after = request(
        &execution,
        "agent.timeline.get.request",
        json!({"agentId":id,"limit":0}),
    )
    .await;
    assert_eq!(after["epoch"], before["epoch"]);
    assert_eq!(after["entries"], before["entries"]);
    request(&execution, "agent.resume.request", json!({"handle":handle})).await;
    request(
        &execution,
        "agent.message.send.request",
        json!({"agentId":id,"text":"wait"}),
    )
    .await;
    request(&execution, "agent.cancel.request", json!({"agentId":id})).await;
    let finished = request(
        &execution,
        "agent.finish.wait.request",
        json!({"agentId":id}),
    )
    .await;
    assert_eq!(finished["status"], "idle");
    execution.shutdown().await.unwrap();
}

async fn approve_and_finish(execution: &AgentExecution, id: &str) {
    let sent = request(
        execution,
        "agent.message.send.request",
        json!({"agentId":id,"text":"hello"}),
    )
    .await;
    assert_eq!(sent["accepted"], true);
    let permission = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let agent = request(execution, "agent.get.request", json!({"agentId":id})).await;
            if let Some(permission) = agent["agent"]["pendingPermissions"]
                .as_array()
                .and_then(|permissions| permissions.first())
            {
                break permission.clone();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    request(
        execution,
        "agent.permission.resolve.request",
        json!({"agentId":id,
        "requestId":permission["id"],"response":{"behavior":"allow","selectedActionId":"once"}}),
    )
    .await;
    let finished = request(
        execution,
        "agent.finish.wait.request",
        json!({"agentId":id}),
    )
    .await;
    assert_eq!(finished["lastMessage"], "Hello world");
}
