use super::{
    ready, start_with_path, terminate,
    transport::{Socket, connect, request},
};
use serde_json::{Value, json};
const METHODS: &[&str] = &[
    "schedule.create.request",
    "schedule.list.request",
    "schedule.inspect.request",
    "schedule.logs.request",
    "schedule.update.request",
    "schedule.pause.request",
    "schedule.resume.request",
    "schedule.delete.request",
    "schedule.run_once.request",
    "agent.get.request",
    "agent.timeline.get.request",
    "agent.create.request",
    "agent.archive.request",
    "workspace.open.request",
    "workspace.get.request",
    "workspace.list.request",
    "workspace.archive.request",
];
async fn success(socket: &mut Socket, method: &str, params: Value) -> Value {
    let reply = request(socket, method, params).await;
    assert_eq!(reply["type"], "response", "{method}: {reply}");
    reply["result"].clone()
}
#[tokio::test]
async fn production_schedule_runs_real_provider_turn_and_preserves_state_across_restart() {
    let fixture = super::native::NativeFixture::new();
    let state = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, METHODS).await;
    let workspace = open_workspace(&mut socket, &fixture.cwd).await;
    let created=success(&mut socket,"schedule.create.request",json!({"workspaceId":workspace,"prompt":"scheduled hello","cadence":{"type":"every","everyMs":3_600_000},"target":{"type":"new-agent","config":{"provider":"codex","cwd":fixture.cwd,"archiveOnFinish":false}},"runOnCreate":false})).await;
    let id = created["schedule"]["id"].clone();
    assert_eq!(created["schedule"]["workspaceId"], workspace);
    assert_eq!(
        success(&mut socket, "schedule.list.request", json!({})).await["schedules"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    success(
        &mut socket,
        "schedule.pause.request",
        json!({"scheduleId":id}),
    )
    .await;
    let ran = success(
        &mut socket,
        "schedule.run_once.request",
        json!({"scheduleId":id}),
    )
    .await;
    assert_eq!(ran["schedule"]["status"], "paused", "{ran}");
    assert_eq!(ran["schedule"]["runs"][0]["status"], "succeeded", "{ran}");
    assert!(ran["schedule"]["runs"][0]["output"].is_string());
    let agent = ran["schedule"]["runs"][0]["agentId"].clone();
    assert_eq!(ran["schedule"]["runs"][0]["workspaceId"], workspace);
    assert_automatic_existing_run(&mut socket, &agent, &workspace).await;
    success(
        &mut socket,
        "schedule.update.request",
        json!({"scheduleId":id,"name":"persisted","maxRuns":3}),
    )
    .await;
    success(
        &mut socket,
        "schedule.resume.request",
        json!({"scheduleId":id}),
    )
    .await;
    assert_eq!(
        success(
            &mut socket,
            "schedule.logs.request",
            json!({"scheduleId":id})
        )
        .await["runs"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    terminate(&mut process).await;
    drop(socket);
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, METHODS).await;
    let record = success(
        &mut socket,
        "schedule.inspect.request",
        json!({"scheduleId":id}),
    )
    .await;
    assert_eq!(record["schedule"]["name"], "persisted");
    assert_eq!(record["schedule"]["runs"][0]["status"], "succeeded");
    success(
        &mut socket,
        "schedule.delete.request",
        json!({"scheduleId":id}),
    )
    .await;
    assert_eq!(
        request(
            &mut socket,
            "schedule.inspect.request",
            json!({"scheduleId":id})
        )
        .await["code"],
        "schedule_request_failed"
    );
    terminate(&mut process).await;
}

async fn assert_automatic_existing_run(socket: &mut Socket, agent: &Value, workspace: &Value) {
    let existing=success(socket,"schedule.create.request",json!({"workspaceId":workspace,"name":"Named existing agent job","prompt":"existing scheduled hello","cadence":{"type":"every","everyMs":3_600_000},"target":{"type":"agent","agentId":agent},"maxRuns":1})).await;
    let existing_id = existing["schedule"]["id"].clone();
    let record = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let record = success(
                socket,
                "schedule.inspect.request",
                json!({"scheduleId":existing_id}),
            )
            .await;
            if record["schedule"]["status"] == "completed" {
                break record;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        record["schedule"]["runs"][0]["status"], "succeeded",
        "{record}"
    );
    let timeline = success(
        socket,
        "agent.timeline.get.request",
        json!({"agentId": agent}),
    )
    .await;
    let schedule_id = existing_id.as_str().unwrap();
    let run_id = record["schedule"]["runs"][0]["id"].as_str().unwrap();
    let expected = format!(
        "<ait-system>\nSchedule \"Named existing agent job\" fired (id={schedule_id}, run={run_id}).\nexisting scheduled hello\n</ait-system>"
    );
    assert!(
        timeline["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["item"]["text"] == expected),
        "{timeline}"
    );
}

async fn open_workspace(socket: &mut Socket, cwd: &std::path::Path) -> Value {
    let result = success(socket, "workspace.open.request", json!({"cwd":cwd})).await;
    assert!(result["workspace"]["id"].is_string(), "{result}");
    result["workspace"]["id"].clone()
}

#[tokio::test]
async fn vanished_schedule_targets_settle_without_creating_agents() {
    let fixture = super::native::NativeFixture::new();
    let state = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, METHODS).await;
    let workspace = open_workspace(&mut socket, &fixture.cwd).await;
    let agent = success(
        &mut socket,
        "agent.create.request",
        json!({"workspaceId":workspace,"config":{"provider":"codex","cwd":fixture.cwd}}),
    )
    .await["agentId"]
        .clone();
    let created = success(&mut socket, "schedule.create.request", json!({"workspaceId":workspace,"prompt":"must not execute","cadence":{"type":"every","everyMs":3_600_000},"target":{"type":"agent","agentId":agent},"runOnCreate":false})).await;
    success(
        &mut socket,
        "agent.archive.request",
        json!({"agentId":agent}),
    )
    .await;
    let ran = success(
        &mut socket,
        "schedule.run_once.request",
        json!({"scheduleId":created["schedule"]["id"]}),
    )
    .await;
    assert_eq!(ran["schedule"]["runs"][0]["status"], "failed", "{ran}");
    assert_eq!(ran["schedule"]["status"], "completed");
    let removed = fixture.root.path().join("removed-workspace");
    std::fs::create_dir(&removed).unwrap();
    let workspace = open_workspace(&mut socket, &removed).await;
    let created = success(&mut socket, "schedule.create.request", json!({"workspaceId":workspace,"prompt":"must not execute","cadence":{"type":"every","everyMs":3_600_000},"target":{"type":"new-agent","config":{"provider":"codex"}},"runOnCreate":false})).await;
    std::fs::remove_dir(&removed).unwrap();
    let ran = success(
        &mut socket,
        "schedule.run_once.request",
        json!({"scheduleId":created["schedule"]["id"]}),
    )
    .await;
    assert_eq!(ran["schedule"]["runs"][0]["status"], "failed", "{ran}");
    assert_eq!(
        ran["schedule"]["runs"][0]["error"], "Scheduled workspace directory no longer exists",
        "{ran}"
    );
    assert!(ran["schedule"]["runs"][0]["agentId"].is_null());
    assert_eq!(ran["schedule"]["runs"][0]["workspaceId"], workspace);
    assert_eq!(ran["schedule"]["status"], "completed");
    terminate(&mut process).await;
}

#[tokio::test]
async fn schedule_archives_created_agent_and_preserves_owning_workspace() {
    let fixture = super::native::NativeFixture::new();
    let state = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, METHODS).await;
    let workspace = open_workspace(&mut socket, &fixture.cwd).await;
    let created=success(&mut socket,"schedule.create.request",json!({"workspaceId":workspace,"prompt":"workspace job","cadence":{"type":"every","everyMs":3_600_000},"target":{"type":"new-agent","config":{"provider":"codex","cwd":"/ignored-client-directory"}},"runOnCreate":false})).await;
    let ran = success(
        &mut socket,
        "schedule.run_once.request",
        json!({"scheduleId":created["schedule"]["id"]}),
    )
    .await;
    assert_eq!(ran["schedule"]["runs"][0]["status"], "succeeded", "{ran}");
    let agent = success(
        &mut socket,
        "agent.get.request",
        json!({"agentId":ran["schedule"]["runs"][0]["agentId"]}),
    )
    .await;
    assert!(agent["agent"]["archivedAt"].is_string(), "{agent}");
    assert_eq!(
        agent["agent"]["cwd"],
        json!(fixture.cwd.canonicalize().unwrap())
    );
    assert_eq!(agent["agent"]["workspaceId"], workspace);
    assert_eq!(
        success(
            &mut socket,
            "workspace.get.request",
            json!({"workspaceId":workspace})
        )
        .await["state"],
        "ready"
    );
    assert_eq!(
        success(&mut socket, "workspace.list.request", json!({})).await["entries"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    terminate(&mut process).await;
}

#[tokio::test]
async fn permission_wait_fails_and_restart_recovers_agent_without_archiving_owner() {
    let fixture = super::native::NativeFixture::new();
    let state = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, METHODS).await;
    let workspace = open_workspace(&mut socket, &fixture.cwd).await;
    let created=success(&mut socket,"schedule.create.request",json!({"workspaceId":workspace,"prompt":"permit-command","cadence":{"type":"every","everyMs":3_600_000},"target":{"type":"new-agent","config":{"provider":"codex","cwd":fixture.cwd,"archiveOnFinish":false}},"runOnCreate":false})).await;
    let id = created["schedule"]["id"].clone();
    let ran = success(
        &mut socket,
        "schedule.run_once.request",
        json!({"scheduleId":id}),
    )
    .await;
    assert_eq!(ran["schedule"]["runs"][0]["status"], "failed", "{ran}");
    assert!(
        ran["schedule"]["runs"][0]["error"]
            .as_str()
            .unwrap()
            .contains("permission"),
        "{ran}"
    );
    terminate(&mut process).await;
    drop(socket);
    // Reproduce a crash after identity checkpoint, before final settlement.
    let path = state.join("schedules/schedules.json");
    let mut doc: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    doc["schedules"][0]["runs"][0]["status"] = json!("running");
    doc["schedules"][0]["runs"][0]["endedAt"] = Value::Null;
    doc["schedules"][0]["target"]["config"]["archiveOnFinish"] = json!(true);
    std::fs::write(&path, doc.to_string()).unwrap();
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, METHODS).await;
    let restored = success(
        &mut socket,
        "schedule.inspect.request",
        json!({"scheduleId":id}),
    )
    .await;
    assert_eq!(restored["schedule"]["runs"][0]["status"], "failed");
    assert!(
        restored["schedule"]["runs"][0]["error"]
            .as_str()
            .unwrap()
            .contains("restarted")
    );
    let agent = success(
        &mut socket,
        "agent.get.request",
        json!({"agentId":ran["schedule"]["runs"][0]["agentId"]}),
    )
    .await;
    assert!(agent["agent"]["archivedAt"].is_string());
    assert_eq!(
        success(
            &mut socket,
            "workspace.get.request",
            json!({"workspaceId":workspace})
        )
        .await["state"],
        "ready"
    );
    terminate(&mut process).await;
}

#[path = "schedule/ownership.rs"]
mod ownership;
