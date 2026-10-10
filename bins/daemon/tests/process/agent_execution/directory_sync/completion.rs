//! Sidebar completion is delivered without opening or subscribing to the Agent timeline.

use super::*;

async fn status_pair(observer: &mut Socket, bucket: &str) -> (Value, Value) {
    let mut workspace = None;
    let mut agent = None;
    while workspace.is_none() || agent.is_none() {
        let event = receive(observer).await;
        let update = &event["params"];
        if event["method"] == "workspace.update" && update["workspace"]["status"] == bucket {
            workspace = Some(update["workspace"].clone());
        }
        if event["method"] == "agent.update"
            && update["agent"]["status"]
                == if bucket == "running" {
                    "running"
                } else {
                    "idle"
                }
        {
            agent = Some(update["agent"].clone());
        }
    }
    (workspace.unwrap(), agent.unwrap())
}

#[tokio::test]
async fn websocket_workspace_running_marker_stops_on_provider_completion_and_cancellation() {
    let super::super::super::native::NativeFixture { root, cwd, path } =
        super::super::super::native::NativeFixture::new();
    let state = root.path().join("state");
    let log = root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&path));
    let address = ready(&mut process, &log).await;
    let methods = directory_methods();
    let mut writer = connect(&address, &methods).await;
    let opened = request(&mut writer, "workspace.open.request", json!({"cwd":cwd})).await;
    let workspace_id = opened["result"]["workspace"]["id"].clone();
    let created = request(
        &mut writer,
        "agent.create.request",
        json!({
            "workspaceId":workspace_id,"config":{"provider":"codex","cwd":cwd}
        }),
    )
    .await;
    let id = created["result"]["agentId"].clone();
    assert!(id.is_string(), "{created}");
    let mut observer = connect(&address, &methods).await;
    ask(
        &mut observer,
        "workspace.list.request",
        json!({"subscribe":{},"sync":{}}),
    )
    .await;
    ask(
        &mut observer,
        "agent.list.request",
        json!({"scope":"active","subscribe":{},"sync":{}}),
    )
    .await;

    for cancel in [false, true] {
        request(
            &mut writer,
            "agent.message.send.request",
            json!({"agentId":id,"text":"hang"}),
        )
        .await;
        let (workspace, running) = status_pair(&mut observer, "running").await;
        assert_eq!(workspace["id"], workspace_id);
        assert_eq!(running["id"], id);
        assert!(running["activeTurn"]["turnId"].is_string());
        if cancel {
            request(&mut writer, "agent.cancel.request", json!({"agentId":id})).await;
        } else {
            request(
                &mut writer,
                "agent.message.send.request",
                json!({
                    "agentId":id,"text":"finished","activeTurnBehavior":"steer"
                }),
            )
            .await;
        }
        let result = request(
            &mut writer,
            "agent.finish.wait.request",
            json!({"agentId":id}),
        )
        .await;
        assert_eq!(result["result"]["status"], "idle", "{result}");
        let (workspace, finished) =
            status_pair(&mut observer, if cancel { "done" } else { "attention" }).await;
        assert_eq!(workspace["id"], workspace_id);
        assert_eq!(finished.get("activeTurn"), Some(&Value::Null));
        assert_eq!(finished["requiresAttention"], !cancel);
    }
    writer.close(None).await.unwrap();
    observer.close(None).await.unwrap();
    terminate(&mut process).await;
    assert_children_exited(&cwd).await;
}
