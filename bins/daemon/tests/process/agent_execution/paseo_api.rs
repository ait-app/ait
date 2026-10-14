//! Real host wiring for Paseo workspace activity and projected timeline responses.

use super::*;

#[tokio::test]
async fn websocket_workspace_status_tracks_native_execution_and_attention() {
    let super::super::native::NativeFixture { root, cwd, path } =
        super::super::native::NativeFixture::new();
    let state = root.path().join("state");
    let log = root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&path));
    let address = ready(&mut process, &log).await;
    let mut methods = METHODS.to_vec();
    methods.extend([
        "workspace.list.request",
        "workspace.clear_attention.request",
        "agent.timeline.get.request",
    ]);
    let mut client = connect(&address, &methods).await;
    let workspace = request(&mut client, "workspace.open.request", json!({"cwd":cwd})).await;
    assert_eq!(workspace["type"], "response", "{workspace}");
    let created = request(
        &mut client,
        "agent.create.request",
        json!({"workspaceId":workspace["result"]["workspace"]["id"],"config":{"provider":"codex","cwd":cwd,"title":"Activity projection"}}),
    )
    .await;
    assert_eq!(created["type"], "response", "{created}");
    let id = created["result"]["agentId"].as_str().unwrap();
    assert_workspace_status(&mut client, "done").await;

    let sent = request(
        &mut client,
        "agent.message.send.request",
        json!({"agentId":id,"text":"hello"}),
    )
    .await;
    assert_eq!(sent["result"]["accepted"], true, "{sent}");
    let finished = request(
        &mut client,
        "agent.finish.wait.request",
        json!({"agentId":id}),
    )
    .await;
    assert_eq!(finished["result"]["status"], "idle", "{finished}");
    let workspace_id = assert_workspace_status(&mut client, "attention").await;
    let cleared = request(
        &mut client,
        "workspace.clear_attention.request",
        json!({"workspaceId":[workspace_id]}),
    )
    .await;
    assert_eq!(cleared["type"], "response", "{cleared}");
    assert_workspace_status(&mut client, "done").await;

    let timeline = request(
        &mut client,
        "agent.timeline.get.request",
        json!({"agentId":id,"projection":"canonical","limit":0}),
    )
    .await;
    assert_eq!(timeline["result"]["projection"], "projected", "{timeline}");
    assert!(
        timeline["result"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["item"]["text"] == "Echo: hello")
    );

    let sent = request(
        &mut client,
        "agent.message.send.request",
        json!({"agentId":id,"text":"hang"}),
    )
    .await;
    assert_eq!(sent["result"]["accepted"], true, "{sent}");
    assert_workspace_status(&mut client, "running").await;
    request(&mut client, "agent.cancel.request", json!({"agentId":id})).await;
    request(
        &mut client,
        "agent.finish.wait.request",
        json!({"agentId":id}),
    )
    .await;
    client.close(None).await.unwrap();
    terminate(&mut process).await;
    assert_children_exited(&cwd).await;
}

async fn assert_workspace_status(
    client: &mut super::super::transport::Socket,
    status: &str,
) -> String {
    let listed = request(client, "workspace.list.request", json!({})).await;
    assert_eq!(listed["type"], "response", "{listed}");
    let workspaces = listed["result"]["entries"].as_array().unwrap();
    assert_eq!(workspaces.len(), 1, "{listed}");
    assert_eq!(workspaces[0]["status"], status, "{listed}");
    assert!(
        workspaces[0]["statusEnteredAt"].as_str().is_some(),
        "{listed}"
    );
    workspaces[0]["id"].as_str().unwrap().to_owned()
}
