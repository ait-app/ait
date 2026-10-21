//! Host crash recovery before accepting new client requests.

use serde_json::json;

use super::transport::{Socket, connect, request};
use super::{ready, start_with_path, terminate};

const METHODS: &[&str] = &[
    "workspace.open.request",
    "agent.create.request",
    "agent.message.send.request",
    "agent.finish.wait.request",
    "agent.timeline.get.request",
    "agent.get.request",
];

#[tokio::test]
async fn opencode_daemon_crash_retires_the_interrupted_turn_before_serving_clients() {
    let fixture = fixture();
    let cwd = fixture.cwd.canonicalize().unwrap();
    std::fs::write(fixture.root.path().join("scenario"), "waiting").unwrap();
    let state = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, METHODS).await;
    request(&mut socket, "workspace.open.request", json!({"cwd":cwd})).await;
    let created = request(
        &mut socket,
        "agent.create.request",
        json!({
            "config":{"provider":"opencode","cwd":cwd,"model":"local/model"}
        }),
    )
    .await;
    let id = created["result"]["agentId"].as_str().unwrap().to_owned();
    let sent = request(
        &mut socket,
        "agent.message.send.request",
        json!({
            "agentId":id,"text":"interrupted work","messageId":"interrupted-input"
        }),
    )
    .await;
    assert_eq!(sent["result"]["accepted"], true, "{sent}");
    waiting_writer(fixture.root.path()).await;
    let active = request(&mut socket, "agent.get.request", json!({"agentId":id})).await;
    assert_eq!(active["result"]["agent"]["status"], "running", "{active}");
    assert!(
        active["result"]["agent"]["runtimeInfo"].is_object(),
        "{active}"
    );
    assert_eq!(
        active["result"]["agent"]["runtimeInfo"]["sessionId"],
        created["result"]["agent"]["persistence"]["sessionId"]
    );
    // SIGKILL bypasses the ordinary close/terminal handshake, as a cgroup OOM kill does.
    process.0.kill().unwrap();
    process.0.wait().unwrap();
    drop(socket);
    std::fs::write(fixture.root.path().join("scenario"), "normal").unwrap();
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, METHODS).await;
    let restored = request(&mut socket, "agent.get.request", json!({"agentId":id})).await;
    assert_eq!(restored["result"]["agent"]["status"], "error", "{restored}");
    assert_eq!(
        restored["result"]["agent"]["runtimeInfo"],
        active["result"]["agent"]["runtimeInfo"]
    );
    assert!(restored["result"]["agent"]["activeTurn"].is_null());
    assert!(
        restored["result"]["agent"]["lastError"]
            .as_str()
            .unwrap()
            .contains("host restarted")
    );
    let requests = std::fs::read_to_string(fixture.root.path().join("requests.jsonl")).unwrap();
    assert_eq!(
        requests
            .lines()
            .filter(|line| line.contains("session/prompt"))
            .count(),
        1
    );
    let finished = request(
        &mut socket,
        "agent.finish.wait.request",
        json!({"agentId":id}),
    )
    .await;
    assert_eq!(finished["result"]["status"], "error", "{finished}");
    let cached = request(
        &mut socket,
        "agent.timeline.get.request",
        json!({
            "agentId":id,"allowCached":true
        }),
    )
    .await;
    assert!(cached.to_string().contains("host restarted"), "{cached}");
    continue_to_completion(&mut socket, &id, "after-restart").await;
    let replayed = request(
        &mut socket,
        "agent.timeline.get.request",
        json!({"agentId":id}),
    )
    .await;
    assert!(
        replayed.to_string().contains("host restarted"),
        "{replayed}"
    );
    terminate(&mut process).await;
    assert_helpers_stopped(fixture.root.path()).await;
}

async fn continue_to_completion(socket: &mut Socket, id: &str, message_id: &str) {
    let sent = request(
        socket,
        "agent.message.send.request",
        json!({"agentId":id,"text":"Continue","messageId":message_id}),
    )
    .await;
    assert_eq!(sent["result"]["accepted"], true, "{sent}");
    let finished = request(socket, "agent.finish.wait.request", json!({"agentId":id})).await;
    assert_eq!(finished["result"]["status"], "idle", "{finished}");
    assert_eq!(finished["result"]["lastMessage"], "authoritative answer");
}

async fn waiting_writer(root: &std::path::Path) -> u32 {
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

fn fixture() -> super::native::NativeFixture {
    let fixture = super::native::NativeFixture::new();
    let program = fixture.root.path().join("opencode");
    std::os::unix::fs::symlink(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/provider/tests/fixtures/opencode_acp.py"),
        &program,
    )
    .unwrap();
    fixture
}

async fn assert_helpers_stopped(root: &std::path::Path) {
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
