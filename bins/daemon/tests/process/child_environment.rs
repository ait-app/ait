use std::os::unix::fs::PermissionsExt;

use serde_json::json;

use super::transport::{connect, request};
use super::{CREDENTIAL_SENTINEL, TOKEN, ready, start_with_environment, terminate};

const METHODS: &[&str] = &[
    "workspace.open.request",
    "agent.create.request",
    "agent.message.send.request",
    "agent.finish.wait.request",
];

#[tokio::test]
async fn native_agents_never_inherit_server_or_runtime_credentials() {
    let fixture = super::native::NativeFixture::new();
    let root = fixture.root.path();
    let peer = root.join("claude_code.py");
    std::fs::write(
        &peer,
        include_str!("../../../../crates/provider/tests/fixtures/claude_code.py"),
    )
    .expect("write Claude peer");
    let dump = root.join("claude-environment.txt");
    let program = root.join("claude");
    std::fs::write(
        &program,
        format!(
            "#!/bin/sh\nenv >> '{}'\nexec /usr/bin/env python3 '{}' \"$@\"\n",
            dump.display(),
            peer.display()
        ),
    )
    .expect("write Claude wrapper");
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700))
        .expect("make wrapper executable");
    let state = root.join("state");
    let log = root.join("server.log");
    // A lone BONSAI_RUNTIME_ variable leaves the adapter disabled but is still private.
    let mut process = start_with_environment(
        &state,
        &log,
        Some(&fixture.path),
        &[("BONSAI_RUNTIME_PROBE", "runtime-probe-value")],
    );
    let address = ready(&mut process, &log).await;
    let mut client = connect(&address, METHODS).await;
    request(
        &mut client,
        "workspace.open.request",
        json!({"cwd": fixture.cwd}),
    )
    .await;
    let created = request(
        &mut client,
        "agent.create.request",
        json!({"config": {"provider": "claude", "cwd": fixture.cwd, "model": "sonnet", "modeId": "default"}}),
    )
    .await;
    let id = created["result"]["agentId"]
        .as_str()
        .unwrap_or_else(|| panic!("{created}"))
        .to_owned();
    let sent = request(
        &mut client,
        "agent.message.send.request",
        json!({"agentId": id, "text": "first"}),
    )
    .await;
    assert_eq!(sent["result"]["accepted"], true, "{sent}");
    request(
        &mut client,
        "agent.finish.wait.request",
        json!({"agentId": id}),
    )
    .await;
    terminate(&mut process).await;
    let environment = std::fs::read_to_string(&dump).expect("the wrapper ran");
    assert!(
        environment.contains("CLAUDE_CONFIG_DIR="),
        "explicit settings still apply"
    );
    for line in environment.lines() {
        assert!(!line.starts_with("BONSAI_RUNTIME_"), "{line}");
        assert!(!line.starts_with("AIT_SERVER_"), "{line}");
    }
    assert!(!environment.contains(TOKEN));
    assert!(!environment.contains(CREDENTIAL_SENTINEL));
}
