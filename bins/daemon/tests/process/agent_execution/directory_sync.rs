//! Real WebSocket directory synchronization, stream ownership and reconnects.

use super::super::transport::Socket;
use super::*;
use futures_util::StreamExt;

#[path = "directory_sync/completion.rs"]
mod completion;

#[tokio::test]
async fn websocket_directory_streams_keep_sequences_ownership_and_reconnect_checkpoints() {
    let super::super::native::NativeFixture { root, cwd, path } =
        super::super::native::NativeFixture::new();
    let state = root.path().join("state");
    let log = root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&path));
    let address = ready(&mut process, &log).await;
    let methods = directory_methods();
    let mut writer = connect(&address, &methods).await;
    assert_directory_features(&mut writer).await;
    let workspace = request(&mut writer, "workspace.open.request", json!({"cwd":cwd})).await;
    let workspace_id = workspace["result"]["workspace"]["id"].as_str().unwrap();
    let created = request(
        &mut writer,
        "agent.create.request",
        json!({"workspaceId":workspace_id,"config":{"provider":"codex","cwd":cwd,"title":"Directory stream"}}),
    )
    .await;
    assert_eq!(created["type"], "response", "{created}");
    let mut observer = connect(&address, &methods).await;
    let workspaces = ask(
        &mut observer,
        "workspace.list.request",
        json!({"subscribe":{},"sync":{}}),
    )
    .await;
    let agents = ask(
        &mut observer,
        "agent.list.request",
        json!({"scope":"active","subscribe":{},"sync":{}}),
    )
    .await;
    let workspace_subscription = workspaces["result"]["subscriptionId"].as_str().unwrap();
    let agent_subscription = agents["result"]["subscriptionId"].as_str().unwrap();
    assert_eq!(
        workspaces["result"]["sync"]["generation"],
        agents["result"]["sync"]["generation"]
    );
    request(
        &mut writer,
        "subscription.release.request",
        json!({"subscriptionId":workspace_subscription}),
    )
    .await;
    request(
        &mut writer,
        "workspace.title.set.request",
        json!({"workspaceId":workspace_id,"title":"First rename"}),
    )
    .await;
    let updates =
        directory_updates(&mut observer, workspace_subscription, agent_subscription).await;
    assert_eq!(updates.0["workspace"]["name"], "First rename");
    assert_eq!(updates.1["project"]["workspaceName"], "First rename");
    assert_eq!(
        updates.0["generation"],
        workspaces["result"]["sync"]["generation"]
    );
    assert!(
        updates.0["seq"].as_u64().unwrap()
            > workspaces["result"]["sync"]["headSeq"].as_u64().unwrap()
    );
    assert_release_stops_workspace(
        &mut observer,
        &mut writer,
        workspace_subscription,
        agent_subscription,
        workspace_id,
    )
    .await;
    observer.close(None).await.unwrap();
    request(
        &mut writer,
        "workspace.title.set.request",
        json!({"workspaceId":workspace_id,"title":"After reconnect"}),
    )
    .await;
    let mut reconnected = connect(&address, &methods).await;
    let caught_up = ask(
        &mut reconnected,
        "workspace.list.request",
        json!({"sync":{
            "generation":updates.0["generation"],"afterSeq":updates.0["seq"]
        }}),
    )
    .await;
    assert_eq!(
        caught_up["result"]["sync"]["mode"], "changes",
        "{caught_up}"
    );
    assert_eq!(caught_up["result"]["entries"][0]["name"], "After reconnect");
    writer.close(None).await.unwrap();
    reconnected.close(None).await.unwrap();
    terminate(&mut process).await;
    assert_children_exited(&cwd).await;
}

async fn assert_directory_features(writer: &mut Socket) {
    let info = request(writer, "server.info", json!({})).await;
    let features = info["result"]["features"].as_array().unwrap();
    for feature in ["directory-sync-v1", "directory-subscriptions-v1"] {
        assert!(features.contains(&json!(feature)), "{info}");
    }
}

fn directory_methods() -> Vec<&'static str> {
    let mut methods = METHODS.to_vec();
    methods.extend([
        "server.info",
        "workspace.list.request",
        "agent.list.request",
        "project.list.request",
        "workspace.title.set.request",
        "subscription.release.request",
    ]);
    methods
}

async fn assert_release_stops_workspace(
    observer: &mut Socket,
    writer: &mut Socket,
    workspace_subscription: &str,
    agent_subscription: &str,
    workspace_id: &str,
) {
    ask(
        observer,
        "subscription.release.request",
        json!({"subscriptionId":workspace_subscription}),
    )
    .await;
    request(
        writer,
        "workspace.title.set.request",
        json!({"workspaceId":workspace_id,"title":"After release"}),
    )
    .await;
    let event = receive(observer).await;
    assert_eq!(
        event["params"]["subscriptionId"], agent_subscription,
        "{event}"
    );
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(600), observer.next())
            .await
            .is_err()
    );
}

async fn ask(socket: &mut Socket, method: &str, params: Value) -> Value {
    socket
        .send(Message::Text(
            json!({"type":"request","request_id":"directory","method":method,"params":params})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    loop {
        let value = receive(socket).await;
        if value["request_id"] == "directory" {
            return value;
        }
        assert_eq!(value["type"], "event", "{value}");
    }
}

async fn directory_updates(socket: &mut Socket, workspace: &str, agent: &str) -> (Value, Value) {
    let mut workspace_update = None;
    let mut agent_update = None;
    while workspace_update.is_none() || agent_update.is_none() {
        let event = receive(socket).await;
        assert_eq!(event["type"], "event", "{event}");
        if event["params"]["subscriptionId"] == workspace {
            workspace_update = Some(event["params"].clone());
        }
        if event["params"]["subscriptionId"] == agent {
            agent_update = Some(event["params"].clone());
        }
    }
    (workspace_update.unwrap(), agent_update.unwrap())
}
