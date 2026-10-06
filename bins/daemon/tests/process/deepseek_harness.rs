//! Real server registration, ACP permission replies, timeline persistence and restart recovery.

use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use serde_json::{Value, json};

use super::transport::{Socket, connect, request};
use super::{ready, start_with_path, terminate};

const METHODS: &[&str] = &[
    "provider.snapshot.get.request",
    "provider.models.list.request",
    "workspace.open.request",
    "agent.create.request",
    "agent.get.request",
    "agent.message.send.request",
    "agent.permission.resolve.request",
    "agent.finish.wait.request",
    "agent.timeline.get.request",
    "agent.resume.request",
    "agent.cancel.request",
];

#[tokio::test]
async fn deepseek_harness_acp_executes_and_keeps_history_after_server_restart() {
    let root = tempfile::tempdir().unwrap();
    let program = root.path().join("dsh");
    std::fs::write(
        &program,
        include_str!(
            "../../../../crates/provider/src/local/deepseek_harness/tests/fixtures/acp.cjs"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    let cwd = root.path().to_str().unwrap();
    let mut paths = vec![root.path().to_path_buf()];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
    let path = std::env::join_paths(paths).unwrap();
    let state = root.path().join("state");
    let log = root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&path));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, METHODS).await;
    success(
        &mut socket,
        "provider.models.list.request",
        json!({"provider":"deepseek-harness","cwd":cwd}),
    )
    .await;
    let snapshot = success(
        &mut socket,
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
    success(&mut socket, "workspace.open.request", json!({"cwd":cwd})).await;
    let created = success(
        &mut socket,
        "agent.create.request",
        json!({"config":{
        "provider":"deepseek-harness","cwd":cwd,"title":"ACP process test"}}),
    )
    .await;
    let id = created["agentId"].as_str().unwrap();
    let handle = &created["agent"]["persistence"];
    approve_and_finish(&mut socket, id).await;
    let before = success(
        &mut socket,
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
    socket.close(None).await.unwrap();
    terminate(&mut process).await;

    let mut process = start_with_path(&state, &log, Some(&path));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, METHODS).await;
    let after = success(
        &mut socket,
        "agent.timeline.get.request",
        json!({"agentId":id,"limit":0}),
    )
    .await;
    assert_eq!(after["epoch"], before["epoch"]);
    assert_eq!(after["entries"], before["entries"]);
    success(
        &mut socket,
        "agent.resume.request",
        json!({"handle":handle}),
    )
    .await;
    success(
        &mut socket,
        "agent.message.send.request",
        json!({"agentId":id,"text":"wait"}),
    )
    .await;
    success(&mut socket, "agent.cancel.request", json!({"agentId":id})).await;
    let finished = success(
        &mut socket,
        "agent.finish.wait.request",
        json!({"agentId":id}),
    )
    .await;
    assert_eq!(finished["status"], "idle");
    socket.close(None).await.unwrap();
    terminate(&mut process).await;
}

async fn success(socket: &mut Socket, method: &str, params: Value) -> Value {
    let response = request(socket, method, params).await;
    assert_eq!(response["type"], "response", "{method}: {response}");
    response["result"].clone()
}

async fn approve_and_finish(socket: &mut Socket, id: &str) {
    let sent = success(
        socket,
        "agent.message.send.request",
        json!({"agentId":id,"text":"hello"}),
    )
    .await;
    assert_eq!(sent["accepted"], true);
    let permission = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let agent = success(socket, "agent.get.request", json!({"agentId":id})).await;
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
    success(
        socket,
        "agent.permission.resolve.request",
        json!({"agentId":id,
        "requestId":permission["id"],"response":{"behavior":"allow","selectedActionId":"once"}}),
    )
    .await;
    let finished = success(socket, "agent.finish.wait.request", json!({"agentId":id})).await;
    assert_eq!(finished["lastMessage"], "Hello world");
}
