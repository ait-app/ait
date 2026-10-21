//! Post-SIGKILL recovery through the real daemon and an offline native Codex peer.

use std::time::Duration;

use serde_json::{Value, json};

use super::transport::{Socket, connect, request};
use super::{Process, ready, start_with_path, terminate};

const METHODS: &[&str] = &[
    "workspace.open.request",
    "agent.create.request",
    "agent.message.send.request",
    "agent.finish.wait.request",
    "agent.get.request",
    "agent.timeline.get.request",
];

#[tokio::test]
async fn codex_oom_restart_continues_the_saved_task_without_replaying_the_original_prompt() {
    restart_case("normal").await;
}

#[tokio::test]
async fn codex_oom_restart_reports_rejected_recovery_and_can_continue_after_a_retry() {
    restart_case("reject-first-input").await;
}

async fn restart_case(mode: &str) {
    let fixture = super::native::NativeFixture::new();
    let state = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, METHODS).await;
    let (id, handle) = interrupt(&fixture, &mut process, &mut socket).await;
    drop(socket);
    std::fs::write(fixture.cwd.join("behavior"), mode).unwrap();

    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, METHODS).await;
    let outcome = request(
        &mut socket,
        "agent.finish.wait.request",
        json!({"agentId":id,"timeoutMs":10000}),
    )
    .await;
    let snapshot = request(&mut socket, "agent.get.request", json!({"agentId":id})).await;
    assert_eq!(
        snapshot["result"]["agent"]["runtimeInfo"]["sessionId"], handle["sessionId"],
        "{snapshot}"
    );
    assert!(snapshot["result"]["agent"]["activeTurn"].is_null());
    let page = request(
        &mut socket,
        "agent.timeline.get.request",
        json!({"agentId":id,"allowCached":true}),
    )
    .await;
    if mode == "normal" {
        assert_eq!(outcome["result"]["status"], "idle", "{outcome}");
        assert!(
            outcome["result"]["lastMessage"]
                .as_str()
                .unwrap()
                .contains("host restarted unexpectedly")
        );
        assert!(snapshot["result"]["agent"]["lastError"].is_null());
    } else {
        assert_eq!(outcome["result"]["status"], "error", "{outcome}");
        let message = snapshot["result"]["agent"]["lastError"].as_str().unwrap();
        assert!(message.contains("sensitive native error"), "{message}");
        assert_eq!(outcome["result"]["error"], message);
        assert!(page.to_string().contains(message), "{page}");
    }
    let requests = std::fs::read_to_string(fixture.cwd.join("native-requests.jsonl")).unwrap();
    let original_count = requests
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|request| {
            request["method"] == "turn/start" && request["params"]["input"][0]["text"] == "hang"
        })
        .count();
    assert_eq!(original_count, 1);
    terminate(&mut process).await;
    std::fs::write(fixture.cwd.join("behavior"), "normal").unwrap();
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, METHODS).await;
    let sent = request(
        &mut socket,
        "agent.message.send.request",
        json!({"agentId":id,"text":"Continue","messageId":"retry-after-recovery"}),
    )
    .await;
    assert_eq!(sent["result"]["accepted"], true, "{sent}");
    let outcome = request(
        &mut socket,
        "agent.finish.wait.request",
        json!({"agentId":id,"timeoutMs":10000}),
    )
    .await;
    assert_eq!(outcome["result"]["status"], "idle", "{outcome}");
    assert_eq!(outcome["result"]["lastMessage"], "Echo: Continue");
    terminate(&mut process).await;
}

async fn interrupt(
    fixture: &super::native::NativeFixture,
    process: &mut Process,
    socket: &mut Socket,
) -> (String, Value) {
    request(socket, "workspace.open.request", json!({"cwd":fixture.cwd})).await;
    let created = request(
        socket,
        "agent.create.request",
        json!({"config":{"provider":"codex","cwd":fixture.cwd}}),
    )
    .await;
    let id = created["result"]["agentId"].as_str().unwrap().to_owned();
    let handle = created["result"]["agent"]["persistence"].clone();
    let sent = request(
        socket,
        "agent.message.send.request",
        json!({"agentId":id,"text":"hang","messageId":"oom-interrupted-input"}),
    )
    .await;
    assert_eq!(sent["result"]["accepted"], true, "{sent}");
    let active = request(socket, "agent.get.request", json!({"agentId":id})).await;
    assert_eq!(active["result"]["agent"]["status"], "running");
    let requests = std::fs::read_to_string(fixture.cwd.join("native-requests.jsonl")).unwrap();
    let writer = requests
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|request| {
            request["method"] == "turn/start" && request["params"]["input"][0]["text"] == "hang"
        })
        .unwrap();
    stop_host(process).await;
    // Reproduce the loss of both the host and its native writer without exhausting host memory.
    assert!(
        std::process::Command::new("/bin/kill")
            .args([
                "-KILL",
                "--",
                &format!("-{}", writer["pid"].as_u64().unwrap())
            ])
            .status()
            .unwrap()
            .success()
    );
    process.0.kill().unwrap();
    process.0.wait().unwrap();
    (id, handle)
}

async fn stop_host(process: &Process) {
    let id = process.0.id().to_string();
    assert!(
        std::process::Command::new("/bin/kill")
            .args(["-STOP", "--", &id])
            .status()
            .unwrap()
            .success()
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let state = std::process::Command::new("/bin/ps")
                .args(["-o", "stat=", "-p", &id])
                .output()
                .unwrap();
            if String::from_utf8_lossy(&state.stdout)
                .trim_start()
                .starts_with('T')
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}
