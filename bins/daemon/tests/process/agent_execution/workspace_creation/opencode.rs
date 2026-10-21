//! `OpenCode` handles through the composite Workspace creation response.

use super::super::super::transport::creation;
use super::*;

#[tokio::test]
async fn opencode_workspace_creation_returns_frontend_compatible_resume_handles() {
    let fixture = super::super::super::native::NativeFixture::new();
    let program = fixture.root.path().join("opencode");
    std::os::unix::fs::symlink(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/provider/tests/fixtures/opencode_acp.py"),
        &program,
    )
    .unwrap();
    let cwd = fixture.cwd.canonicalize().unwrap();
    let state = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, &methods()).await;
    let (created, _) = creation(
        &mut socket,
        "workspace.create.request",
        json!({"idempotencyKey":"opencode-workspace", "subscribe":true,
            "source":{"kind":"directory","path":cwd},
            "agent":{"config":{
                "provider":"opencode","cwd":cwd,"model":"local/model","modeId":"build"
            },"initialPrompt":"hello gui"}}),
    )
    .await;
    assert_eq!(created["type"], "response", "{created}");
    let result = &created["result"];
    assert!(result["error"].is_null(), "{created}");
    assert_eq!(result["creation"]["phase"], "completed");
    for agent in [&result["agent"], &result["creation"]["agent"]] {
        let native = agent["persistence"]["nativeHandle"]
            .as_str()
            .expect("frontend requires an opaque string handle");
        assert_eq!(native, agent["persistence"]["sessionId"]);
        assert_eq!(
            agent["persistence"]["metadata"]["opencode"]["model"],
            "local/model"
        );
    }
    let mut finished = request(
        &mut socket,
        "agent.finish.wait.request",
        json!({"agentId":result["agent"]["id"]}),
    )
    .await;
    while finished["type"] == "event" {
        finished = receive(&mut socket).await;
    }
    assert_eq!(
        finished["result"]["lastMessage"], "authoritative answer",
        "{finished}"
    );
    terminate(&mut process).await;
    assert_helpers_stopped(fixture.root.path()).await;
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
