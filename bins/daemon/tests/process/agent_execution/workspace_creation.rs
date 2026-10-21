//! Composite Workspace creation contracts adapted from Paseo creation and session tests.

use std::path::PathBuf;

use super::super::transport::Socket;
use super::super::worktrees::{create_repository, run};
use super::*;

#[path = "workspace_creation/concurrency.rs"]
mod concurrency;
#[path = "workspace_creation/identity.rs"]
mod identity;
#[path = "workspace_creation/opencode.rs"]
mod opencode;

fn methods() -> Vec<&'static str> {
    let mut methods = METHODS.to_vec();
    methods.extend([
        "workspace.create.request",
        "workspace.list.request",
        "creation.subscribe.request",
        "subscription.release.request",
        "agent.list.request",
        "server.info",
    ]);
    methods
}

async fn call(client: &mut Socket, method: &str, params: Value) -> Value {
    let value = reply(client, method, params).await;
    assert_eq!(value["type"], "response", "{value}");
    value["result"].clone()
}

async fn reply(client: &mut Socket, method: &str, params: Value) -> Value {
    let mut value = request(client, method, params).await;
    while value["type"] == "event" {
        value = receive(client).await;
    }
    value
}

#[tokio::test]
async fn workspace_initial_agent_preserves_subdirectories_receipts_environment_and_first_prompt() {
    let fixture = super::super::native::NativeFixture::new();
    let source_agent = prepare_repository(&fixture);
    let state = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut client = connect(&address, &methods()).await;
    let info = call(&mut client, "server.info", json!({})).await;
    assert!(
        info["features"]
            .as_array()
            .unwrap()
            .contains(&json!("creation-lifecycle-v1"))
    );
    let mut replies = Vec::new();
    for (key, source) in [
        (
            "directory-agent",
            json!({"kind":"directory","path":fixture.cwd}),
        ),
        (
            "worktree-agent",
            json!({"kind":"worktree","cwd":fixture.cwd,"branchName":"composite"}),
        ),
    ] {
        let params = json!({"idempotencyKey":key,"subscribe":true,"source":source,
            "agent":{"config":{"provider":"codex","cwd":source_agent},
                "initialPrompt":"composite hello","env":{"AIT_TEST_AGENT_ENV":"composite-private"}}});
        let (first, progress) = super::super::transport::creation(
            &mut client,
            "workspace.create.request",
            params.clone(),
        )
        .await;
        assert_eq!(first["type"], "response", "{first}");
        let result = &first["result"];
        assert!(result["error"].is_null(), "{result}");
        assert_eq!(result["creation"]["phase"], "completed");
        assert_eq!(result["creation"]["agentId"], result["agent"]["id"]);
        assert_eq!(result["agent"]["workspaceId"], result["workspace"]["id"]);
        let directory = PathBuf::from(result["agent"]["cwd"].as_str().unwrap());
        assert_eq!(
            directory,
            PathBuf::from(result["workspace"]["workspaceDirectory"].as_str().unwrap())
                .join("packages/app")
                .canonicalize()
                .unwrap()
        );
        assert_phases(&progress);
        let waited = call(
            &mut client,
            "agent.finish.wait.request",
            json!({"agentId":result["agent"]["id"]}),
        )
        .await;
        assert_eq!(waited["lastMessage"], "Echo: composite hello");
        assert!(
            std::fs::read_to_string(directory.join("native-environment.jsonl"))
                .unwrap()
                .contains("composite-private")
        );
        let mut retry = params.clone();
        retry["subscribe"] = json!(false);
        let replay = call(&mut client, "workspace.create.request", retry.clone()).await;
        assert_eq!(replay["workspace"]["id"], result["workspace"]["id"]);
        assert_eq!(replay["agent"]["id"], result["agent"]["id"]);
        assert!(result.get("subscriptionId").is_none());
        replies.push((retry, result["agent"]["id"].clone()));
    }
    let listed = call(&mut client, "agent.list.request", json!({})).await;
    assert_eq!(listed["entries"].as_array().unwrap().len(), 2);
    client.close(None).await.unwrap();
    terminate(&mut process).await;
    let receipts = std::fs::read_to_string(state.join("creations/receipts.json")).unwrap();
    assert!(!receipts.contains("composite-private"));
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut client = connect(&address, &methods()).await;
    for (params, id) in replies {
        let result = call(&mut client, "workspace.create.request", params).await;
        assert_eq!(result["agent"]["id"], id);
        assert_eq!(result["creation"]["phase"], "completed");
    }
    terminate(&mut process).await;
}

fn prepare_repository(fixture: &super::super::native::NativeFixture) -> PathBuf {
    create_repository(&fixture.cwd);
    let source_agent = fixture.cwd.join("packages/app");
    std::fs::write(source_agent.join("capture-environment"), "").unwrap();
    run(&fixture.cwd, &["add", "."]);
    run(
        &fixture.cwd,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-m",
            "capture",
        ],
    );
    source_agent
}

#[tokio::test]
async fn invalid_initial_agent_is_rejected_before_provisioning_and_native_failure_keeps_workspace()
{
    let fixture = super::super::native::NativeFixture::new();
    let state = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut client = connect(&address, &methods()).await;
    let base = json!({"idempotencyKey":"invalid","source":{"kind":"directory","path":fixture.cwd},
        "agent":{"config":{"provider":"codex","cwd":fixture.cwd}}});
    let mut invalid = base.clone();
    invalid["agent"]["git"] = json!({});
    assert_eq!(
        request(&mut client, "workspace.create.request", invalid).await["code"],
        "invalid_message"
    );
    assert_eq!(
        call(&mut client, "workspace.list.request", json!({})).await["entries"],
        json!([])
    );
    std::fs::write(fixture.cwd.join("behavior"), "error").unwrap();
    let failed = call(&mut client, "workspace.create.request", base.clone()).await;
    assert!(failed["error"].is_string(), "{failed}");
    assert_eq!(failed["creation"]["phase"], "failed");
    assert!(failed["workspace"]["id"].is_string());
    assert!(failed["agent"].is_null());
    std::fs::remove_file(fixture.cwd.join("behavior")).unwrap();
    let replay = call(&mut client, "workspace.create.request", base).await;
    assert!(replay["error"].is_null(), "{replay}");
    assert_eq!(replay["workspace"]["id"], failed["workspace"]["id"]);
    assert_eq!(replay["agent"]["id"], failed["creation"]["agentId"]);
    assert_eq!(replay["creation"]["phase"], "completed");
    let mut outside = json!({"idempotencyKey":"outside","source":{"kind":"directory","path":fixture.cwd},
        "agent":{"config":{"provider":"codex","cwd":fixture.root.path()}}});
    let failed = call(&mut client, "workspace.create.request", outside.clone()).await;
    assert_eq!(failed["creation"]["phase"], "failed");
    assert!(failed["agent"].is_null());
    outside["agent"]["config"]["cwd"] = json!(fixture.cwd);
    assert_eq!(
        reply(&mut client, "workspace.create.request", outside).await["code"],
        "idempotency_conflict"
    );
    assert_eq!(
        call(&mut client, "agent.list.request", json!({})).await["entries"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    terminate(&mut process).await;
}

fn assert_phases(progress: &[Value]) {
    let mut phases = Vec::new();
    for event in progress {
        assert_eq!(event["method"], "workspace.create.update", "{event}");
        assert!(event["params"].get("subscriptionId").is_none());
        phases.push(event["params"]["phase"].clone());
    }
    assert_eq!(
        phases,
        vec![
            json!("accepted"),
            json!("workspace_ready"),
            json!("agent_ready"),
            json!("prompt_started"),
            json!("completed")
        ]
    );
}
