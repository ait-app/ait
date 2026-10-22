//! Native `OpenCode` discovery, durable import and worker restart contracts.

use std::path::Path;

use serde_json::{Value, json};

use super::super::OpenCodeClient;
use crate::service::agent_execution::AgentExecution;
use crate::test_support::{Fixture, request, worker_with_client};

fn fixture() -> Fixture {
    let fixture = Fixture::new();
    let program = fixture.root.path().join("opencode");
    std::os::unix::fs::symlink(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/opencode_acp.py"),
        &program,
    )
    .unwrap();
    fixture
}

fn worker(fixture: &Fixture) -> AgentExecution {
    let client = OpenCodeClient::new(fixture.root.path().join("opencode"));
    worker_with_client(
        fixture,
        model::creation::Creations::default(),
        Box::new(client),
    )
    .0
}

#[tokio::test]
async fn external_session_import_survives_worker_restart_and_continues() {
    let fixture = fixture();
    let cwd = fixture.cwd.canonicalize().unwrap();
    let native_path = fixture.root.path().join("native-fixture.json");
    let native = json!({
        "seq":1,
        "info":{"id":"ses_one","title":"External session","location":{"directory":cwd},
            "model":{"providerID":"local","id":"model","variant":"default"},
            "time":{"created":10,"updated":12},"permissions":[],"outcome":"succeeded"},
        "history":[
            {"id":"user1","type":"user","text":"external prompt","time":{"created":10}},
            {"id":"answer1","type":"assistant",
                "content":[{"type":"text","text":"external answer"}],
                "time":{"created":11,"completed":12}}
        ]
    });
    std::fs::write(&native_path, native.to_string()).unwrap();
    let execution = worker(&fixture);
    let listed = request(
        &execution,
        "provider.sessions.recent.list.request",
        json!({"providers":["opencode"],"cwd":cwd}),
    )
    .await;
    assert_eq!(
        listed["entries"][0]["providerHandleId"], "ses_one",
        "{listed}"
    );
    let imported = request(
        &execution,
        "agent.import.request",
        json!({"providerId":"opencode","providerHandleId":"ses_one","cwd":cwd}),
    )
    .await;
    let id = imported["agentId"].as_str().unwrap();
    let timeline = request(
        &execution,
        "agent.timeline.get.request",
        json!({"agentId":id}),
    )
    .await;
    assert!(
        timeline.to_string().contains("external answer"),
        "{timeline}"
    );
    let listed = request(
        &execution,
        "provider.sessions.recent.list.request",
        json!({"providers":["opencode"],"cwd":cwd}),
    )
    .await;
    assert_eq!(listed["entries"], json!([]), "{listed}");
    assert_eq!(
        std::fs::read_to_string(&native_path).unwrap(),
        native.to_string()
    );
    execution.shutdown().await.unwrap();
    assert_helpers_stopped(fixture.root.path()).await;

    let execution = worker(&fixture);
    send_and_finish(&execution, id, "continue imported session", None).await;
    execution.shutdown().await.unwrap();
    let native: Value = serde_json::from_slice(&std::fs::read(native_path).unwrap()).unwrap();
    assert_eq!(native["info"]["id"], "ses_one");
    assert_eq!(native["seq"], 2);
    assert_helpers_stopped(fixture.root.path()).await;
}

#[tokio::test]
async fn discovery_execution_and_restore_preserve_native_history_and_string_handles() {
    let fixture = fixture();
    let execution = worker(&fixture);
    let models = request(
        &execution,
        "provider.models.list.request",
        json!({"provider":"opencode","cwd":fixture.cwd}),
    )
    .await;
    assert_eq!(models["models"][0]["id"], "local/model", "{models}");
    assert_eq!(models["models"][0]["provider"], "opencode", "{models}");
    let snapshot = request(
        &execution,
        "provider.snapshot.get.request",
        json!({"cwd":fixture.cwd}),
    )
    .await;
    let entry = snapshot["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["provider"] == "opencode")
        .unwrap();
    assert_eq!(entry["status"], "ready", "{snapshot}");
    assert_eq!(entry["models"][0]["provider"], "opencode", "{snapshot}");
    let available = request(&execution, "provider.available.list.request", json!({})).await;
    assert!(available.to_string().contains("opencode"), "{available}");
    let created = request(
        &execution,
        "agent.create.request",
        json!({"config":{
            "provider":"opencode","cwd":fixture.cwd,"model":"local/model","modeId":"build"
        }}),
    )
    .await;
    let id = created["agentId"].as_str().unwrap();
    let handle = &created["agent"]["persistence"];
    assert_eq!(handle["provider"], "opencode");
    let native = handle["nativeHandle"]
        .as_str()
        .expect("frontend requires a string handle");
    assert_eq!(native, handle["sessionId"]);
    assert_eq!(handle["metadata"]["opencode"]["model"], "local/model");
    for index in 0..2 {
        send_and_finish(
            &execution,
            id,
            &format!("hello {index}"),
            Some(format!("client-{index}")),
        )
        .await;
    }
    execution.shutdown().await.unwrap();
    assert_helpers_stopped(fixture.root.path()).await;

    let execution = worker(&fixture);
    let resumed = request(&execution, "agent.resume.request", json!({"handle":handle})).await;
    assert_eq!(resumed["agentId"], id, "{resumed}");
    send_and_finish(&execution, id, "after restart", None).await;
    let timeline = request(
        &execution,
        "agent.timeline.get.request",
        json!({"agentId":id}),
    )
    .await
    .to_string();
    for text in [
        "hello 0",
        "hello 1",
        "after restart",
        "authoritative answer",
    ] {
        assert!(timeline.contains(text), "{timeline}");
    }
    execution.shutdown().await.unwrap();
    let native: Value = serde_json::from_slice(
        &std::fs::read(fixture.root.path().join("native-fixture.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(native["seq"], 3);
    assert_eq!(native["history"].as_array().unwrap().len(), 6);
    assert_helpers_stopped(fixture.root.path()).await;
}

async fn send_and_finish(
    execution: &AgentExecution,
    id: &str,
    text: &str,
    message_id: Option<String>,
) {
    let sent = request(
        execution,
        "agent.message.send.request",
        json!({"agentId":id,"text":text,"messageId":message_id}),
    )
    .await;
    assert_eq!(sent["accepted"], true, "{sent}");
    let finished = request(
        execution,
        "agent.finish.wait.request",
        json!({"agentId":id}),
    )
    .await;
    assert_eq!(
        finished["lastMessage"], "authoritative answer",
        "{finished}"
    );
}

async fn assert_helpers_stopped(root: &Path) {
    for pid in std::fs::read_to_string(root.join("pids.txt"))
        .unwrap()
        .lines()
    {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let output = std::process::Command::new("ps")
                .args(["-o", "stat=", "-p", pid])
                .output()
                .unwrap();
            assert!(output.status.success() || output.status.code() == Some(1));
            let state = String::from_utf8(output.stdout).unwrap();
            // kill -0 also succeeds for a dead orphan awaiting its Linux parent's reap.
            // A zombie cannot execute work; any runnable helper must stop within the deadline.
            if state.trim().is_empty() || state.trim().starts_with('Z') {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "OpenCode helper {pid} survived shutdown with process state {state:?}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }
}

#[tokio::test]
async fn native_process_exit_finishes_the_turn_and_allows_explicit_continue() {
    let fixture = fixture();
    let cwd = fixture.cwd.canonicalize().unwrap();
    std::fs::write(fixture.root.path().join("scenario"), "waiting").unwrap();
    let execution = worker(&fixture);
    let created = request(
        &execution,
        "agent.create.request",
        json!({"config":{"provider":"opencode","cwd":cwd,"model":"local/model"}}),
    )
    .await;
    let id = created["agentId"].as_str().unwrap().to_owned();
    let sent = request(
        &execution,
        "agent.message.send.request",
        json!({"agentId":id,"text":"interrupted work","messageId":"native-exit"}),
    )
    .await;
    assert_eq!(sent["accepted"], true, "{sent}");
    let pid = waiting_writer(fixture.root.path()).await;
    assert!(
        std::process::Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status()
            .unwrap()
            .success()
    );
    let finished = request(
        &execution,
        "agent.finish.wait.request",
        json!({"agentId":id}),
    )
    .await;
    assert_eq!(finished["status"], "error", "{finished}");
    let snapshot = request(&execution, "agent.get.request", json!({"agentId":id})).await;
    assert_eq!(snapshot["agent"]["status"], "error", "{snapshot}");
    assert!(snapshot["agent"]["activeTurn"].is_null());
    assert!(snapshot["agent"]["lastError"].is_string());
    let cached = request(
        &execution,
        "agent.timeline.get.request",
        json!({"agentId":id,"allowCached":true}),
    )
    .await;
    assert!(
        cached.to_string().contains("\"type\":\"error\""),
        "{cached}"
    );
    std::fs::write(fixture.root.path().join("scenario"), "normal").unwrap();
    continue_to_completion(&execution, &id, "after-native-exit").await;
    execution.shutdown().await.unwrap();
    assert_helpers_stopped(fixture.root.path()).await;
}

async fn continue_to_completion(execution: &AgentExecution, id: &str, message_id: &str) {
    let sent = request(
        execution,
        "agent.message.send.request",
        json!({"agentId":id,"text":"Continue","messageId":message_id}),
    )
    .await;
    assert_eq!(sent["accepted"], true, "{sent}");
    let finished = request(
        execution,
        "agent.finish.wait.request",
        json!({"agentId":id}),
    )
    .await;
    assert_eq!(finished["status"], "idle", "{finished}");
    assert_eq!(finished["lastMessage"], "authoritative answer");
}

async fn waiting_writer(root: &Path) -> u32 {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if let Some(pid) = std::fs::read_to_string(root.join("waiting-pid"))
                .ok()
                .and_then(|pid| pid.parse().ok())
            {
                return pid;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("OpenCode fixture must accept the native turn before the crash")
}
