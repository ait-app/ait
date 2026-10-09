use serde_json::json;

use super::transport::{Socket, connect, request};
use super::{ready, start_with_path, terminate};

const METHODS: &[&str] = &[
    "workspace.open.request",
    "workspace.create.request",
    "agent.create.request",
    "agent.message.send.request",
    "agent.finish.wait.request",
    "agent.timeline.get.request",
    "provider.models.list.request",
    "provider.snapshot.get.request",
    "provider.available.list.request",
    "agent.resume.request",
    "agent.get.request",
    "provider.sessions.recent.list.request",
    "agent.import.request",
];

#[tokio::test]
async fn opencode_external_session_import_survives_daemon_restart_and_continues() {
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
            {"id":"answer1","type":"assistant","content":[{"type":"text","text":"external answer"}],"time":{"created":11,"completed":12}}
        ]
    });
    std::fs::write(&native_path, native.to_string()).unwrap();
    let state = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, METHODS).await;
    let listed = request(
        &mut socket,
        "provider.sessions.recent.list.request",
        json!({"providers":["opencode"],"cwd":cwd}),
    )
    .await;
    assert_eq!(
        listed["result"]["entries"][0]["providerHandleId"], "ses_one",
        "{listed}"
    );
    let imported = request(
        &mut socket,
        "agent.import.request",
        json!({"providerId":"opencode","providerHandleId":"ses_one","cwd":cwd}),
    )
    .await;
    let id = imported["result"]["agentId"]
        .as_str()
        .unwrap_or_else(|| panic!("{imported}"))
        .to_owned();
    let timeline = request(
        &mut socket,
        "agent.timeline.get.request",
        json!({"agentId":id}),
    )
    .await;
    assert!(
        timeline.to_string().contains("external answer"),
        "{timeline}"
    );
    let listed = request(
        &mut socket,
        "provider.sessions.recent.list.request",
        json!({"providers":["opencode"],"cwd":cwd}),
    )
    .await;
    assert_eq!(listed["result"]["entries"], json!([]), "{listed}");
    assert_eq!(
        std::fs::read_to_string(&native_path).unwrap(),
        native.to_string()
    );
    terminate(&mut process).await;
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, METHODS).await;
    let sent = request(
        &mut socket,
        "agent.message.send.request",
        json!({"agentId":id,"text":"continue imported session"}),
    )
    .await;
    assert_eq!(sent["result"]["accepted"], true, "{sent}");
    let finished = request(
        &mut socket,
        "agent.finish.wait.request",
        json!({"agentId":id}),
    )
    .await;
    assert_eq!(
        finished["result"]["lastMessage"], "authoritative answer",
        "{finished}"
    );
    terminate(&mut process).await;
    let native: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&native_path).unwrap()).unwrap();
    assert_eq!(native["info"]["id"], "ses_one");
    assert_eq!(native["seq"], 2);
    assert_helpers_stopped(fixture.root.path()).await;
}

#[tokio::test]
async fn opencode_is_discovered_executed_and_restored_through_the_real_server() {
    let fixture = fixture();
    let cwd = fixture.cwd.canonicalize().unwrap();
    let state = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, METHODS).await;
    assert_discovery(&mut socket, &cwd).await;
    let available = request(&mut socket, "provider.available.list.request", json!({})).await;
    assert!(available.to_string().contains("opencode"), "{available}");
    request(&mut socket, "workspace.open.request", json!({"cwd":cwd})).await;
    let created = request(
        &mut socket,
        "agent.create.request",
        json!({"config":{"provider":"opencode","cwd":cwd,"model":"local/model","modeId":"build"}}),
    )
    .await;
    let id = created["result"]["agentId"]
        .as_str()
        .unwrap_or_else(|| {
            panic!(
                "{created}; providers: {available}; log: {}",
                std::fs::read_to_string(&log).unwrap()
            )
        })
        .to_owned();
    assert_eq!(
        created["result"]["agent"]["persistence"]["provider"],
        "opencode"
    );
    for index in 0..2 {
        let sent = request(&mut socket,"agent.message.send.request",json!({"agentId":id,"text":format!("hello {index}"),"messageId":format!("client-{index}")})).await;
        assert_eq!(sent["result"]["accepted"], true, "{sent}");
        let finished = request(
            &mut socket,
            "agent.finish.wait.request",
            json!({"agentId":id}),
        )
        .await;
        assert_eq!(
            finished["result"]["lastMessage"], "authoritative answer",
            "{finished}"
        );
    }
    terminate(&mut process).await;
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, METHODS).await;
    let resumed = request(
        &mut socket,
        "agent.resume.request",
        json!({"handle":created["result"]["agent"]["persistence"]}),
    )
    .await;
    assert_eq!(resumed["type"], "response", "{resumed}");
    let sent = request(
        &mut socket,
        "agent.message.send.request",
        json!({"agentId":id,"text":"after restart"}),
    )
    .await;
    assert_eq!(sent["result"]["accepted"], true, "{sent}");
    let finished = request(
        &mut socket,
        "agent.finish.wait.request",
        json!({"agentId":id}),
    )
    .await;
    assert_eq!(
        finished["result"]["lastMessage"], "authoritative answer",
        "{finished}"
    );
    let timeline = request(
        &mut socket,
        "agent.timeline.get.request",
        json!({"agentId":id}),
    )
    .await;
    assert_eq!(timeline["type"], "response", "{timeline}");
    terminate(&mut process).await;
    let native: serde_json::Value = serde_json::from_slice(
        &std::fs::read(fixture.root.path().join("native-fixture.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(native["seq"], 3);
    assert_eq!(native["history"].as_array().unwrap().len(), 6);
    assert_helpers_stopped(fixture.root.path()).await;
}

async fn assert_discovery(socket: &mut Socket, cwd: &std::path::Path) {
    let models = request(
        socket,
        "provider.models.list.request",
        json!({"provider":"opencode","cwd":cwd}),
    )
    .await;
    assert_eq!(
        models["result"]["models"][0]["id"], "local/model",
        "{models}"
    );
    assert_eq!(
        models["result"]["models"][0]["provider"], "opencode",
        "{models}"
    );
    let snapshot = request(socket, "provider.snapshot.get.request", json!({"cwd":cwd})).await;
    let entry = snapshot["result"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["provider"] == "opencode")
        .unwrap();
    assert_eq!(entry["status"], "ready", "{snapshot}");
    assert_eq!(entry["models"][0]["provider"], "opencode", "{snapshot}");
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

#[tokio::test]
async fn opencode_workspace_creation_returns_frontend_compatible_resume_handles() {
    let fixture = fixture();
    let cwd = fixture.cwd.canonicalize().unwrap();
    let state = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, METHODS).await;
    let (created, _) = super::transport::creation(&mut socket, "workspace.create.request",
        json!({"idempotencyKey":"opencode-workspace", "subscribe":true,
        "source":{"kind":"directory","path":cwd}, "agent":{"config":{"provider":"opencode","cwd":cwd,"model":"local/model","modeId":"build"},"initialPrompt":"hello gui"}})).await;
    assert_eq!(created["type"], "response", "{created}");
    let result = &created["result"];
    assert!(result["error"].is_null(), "{created}");
    assert_eq!(result["creation"]["phase"], "completed");
    for agent in [&result["agent"], &result["creation"]["agent"]] {
        let encoded = agent["persistence"]["nativeHandle"]
            .as_str()
            .expect("frontend requires an opaque string handle");
        let decoded: serde_json::Value = serde_json::from_str(encoded).unwrap();
        assert_eq!(decoded["model"], "local/model");
    }
    let mut finished = request(
        &mut socket,
        "agent.finish.wait.request",
        json!({"agentId":result["agent"]["id"]}),
    )
    .await;
    while finished["type"] == "event" {
        finished = super::transport::receive(&mut socket).await;
    }
    assert_eq!(
        finished["result"]["lastMessage"], "authoritative answer",
        "{finished}"
    );
    terminate(&mut process).await;
    assert_helpers_stopped(fixture.root.path()).await;
}
