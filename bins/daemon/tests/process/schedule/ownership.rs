//! Workspace-owned schedules across the production WebSocket and native provider adapters.
use super::*;

#[tokio::test]
async fn create_and_update_reject_agents_from_another_workspace() {
    let fixture = super::super::native::NativeFixture::new();
    let state = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, METHODS).await;
    let owner = open_workspace(&mut socket, &fixture.cwd).await;
    let other_cwd = fixture.root.path().join("other");
    std::fs::create_dir(&other_cwd).unwrap();
    let other = open_workspace(&mut socket, &other_cwd).await;
    let agent = success(
        &mut socket,
        "agent.create.request",
        json!({"workspaceId":owner,"config":{"provider":"codex","cwd":fixture.cwd}}),
    )
    .await["agentId"]
        .clone();
    let mut params = json!({"workspaceId":other,"prompt":"run in owner","cadence":{"type":"every","everyMs":3_600_000},"target":{"type":"agent","agentId":agent},"runOnCreate":false});
    assert_eq!(
        request(&mut socket, "schedule.create.request", params.clone()).await["code"],
        "schedule_request_failed"
    );
    assert!(
        success(&mut socket, "schedule.list.request", json!({})).await["schedules"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    params["workspaceId"] = owner.clone();
    let id =
        success(&mut socket, "schedule.create.request", params).await["schedule"]["id"].clone();
    assert_eq!(
        request(
            &mut socket,
            "schedule.update.request",
            json!({"scheduleId":id,"workspaceId":other})
        )
        .await["code"],
        "schedule_request_failed"
    );
    let foreign_agent = success(
        &mut socket,
        "agent.create.request",
        json!({"workspaceId":other,"config":{"provider":"codex","cwd":other_cwd}}),
    )
    .await["agentId"]
        .clone();
    assert_eq!(
        request(
            &mut socket,
            "schedule.update.request",
            json!({"scheduleId":id,"target":{"type":"agent","agentId":foreign_agent}})
        )
        .await["code"],
        "schedule_request_failed"
    );
    let record = success(
        &mut socket,
        "schedule.inspect.request",
        json!({"scheduleId":id}),
    )
    .await;
    assert_eq!(record["schedule"]["workspaceId"], owner);
    assert_eq!(record["schedule"]["target"]["agentId"], agent);
    assert!(
        success(
            &mut socket,
            "schedule.list.request",
            json!({"workspaceId":other})
        )
        .await["schedules"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        success(
            &mut socket,
            "schedule.list.request",
            json!({"workspaceId":owner})
        )
        .await["schedules"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    terminate(&mut process).await;
}

#[tokio::test]
async fn one_time_schedule_fires_automatically_and_cannot_repeat() {
    let fixture = super::super::native::NativeFixture::new();
    let state = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, METHODS).await;
    let owner = open_workspace(&mut socket, &fixture.cwd).await;
    let at = chrono::Utc::now() + chrono::Duration::seconds(2);
    let created = success(&mut socket,"schedule.create.request",json!({"workspaceId":owner,"prompt":"once","cadence":{"type":"once","at":at},"target":{"type":"new-agent","config":{"provider":"codex"}}})).await;
    let id = created["schedule"]["id"].clone();
    let result = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let record = success(
                &mut socket,
                "schedule.inspect.request",
                json!({"scheduleId":id}),
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
    assert_eq!(result["schedule"]["runs"].as_array().unwrap().len(), 1);
    assert_eq!(
        result["schedule"]["runs"][0]["status"], "succeeded",
        "{result}"
    );
    assert_eq!(result["schedule"]["runs"][0]["workspaceId"], owner);
    assert_eq!(
        request(
            &mut socket,
            "schedule.run_once.request",
            json!({"scheduleId":id})
        )
        .await["code"],
        "schedule_request_failed"
    );
    assert_eq!(
        success(
            &mut socket,
            "workspace.get.request",
            json!({"workspaceId":owner})
        )
        .await["state"],
        "ready"
    );
    terminate(&mut process).await;
}

#[tokio::test]
async fn legacy_worktree_schedule_retains_its_placement_and_cleanup_on_version_two_writes() {
    let fixture = super::super::native::NativeFixture::new();
    for args in [
        vec!["init", "-b", "main"],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
    ] {
        assert!(
            std::process::Command::new("git")
                .current_dir(&fixture.cwd)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    let state = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, METHODS).await;
    let owner = open_workspace(&mut socket, &fixture.cwd).await;
    let id = success(&mut socket,"schedule.create.request",json!({"workspaceId":owner,"prompt":"legacy worktree","cadence":{"type":"every","everyMs":3_600_000},"target":{"type":"new-agent","config":{"provider":"codex"}},"runOnCreate":false})).await["schedule"]["id"].clone();
    terminate(&mut process).await;
    drop(socket);
    let path = state.join("schedules/schedules.json");
    let mut doc: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    doc["version"] = json!(1);
    doc["schedules"][0]
        .as_object_mut()
        .unwrap()
        .remove("workspaceId");
    doc["schedules"][0]
        .as_object_mut()
        .unwrap()
        .remove("startAt");
    doc["schedules"][0]["target"]["config"]["isolation"] = json!("worktree");
    std::fs::write(&path, doc.to_string()).unwrap();
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, METHODS).await;
    let ran = success(
        &mut socket,
        "schedule.run_once.request",
        json!({"scheduleId":id}),
    )
    .await;
    assert!(ran["schedule"]["workspaceId"].is_null());
    assert_eq!(ran["schedule"]["runs"][0]["status"], "succeeded", "{ran}");
    let run_workspace = ran["schedule"]["runs"][0]["workspaceId"].clone();
    assert_ne!(run_workspace, owner);
    assert_eq!(
        success(
            &mut socket,
            "workspace.get.request",
            json!({"workspaceId":run_workspace})
        )
        .await["state"],
        "archived"
    );
    assert_eq!(
        success(
            &mut socket,
            "workspace.get.request",
            json!({"workspaceId":owner})
        )
        .await["state"],
        "ready"
    );
    let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["version"], 2);
    terminate(&mut process).await;
}
