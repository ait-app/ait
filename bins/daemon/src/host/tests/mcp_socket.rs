use super::*;

use serde_json::{Value, json};

const TOKEN: &str = "offline-host-test-token-at-least-32-characters";

#[tokio::test]
async fn mcp_socket_serves_the_shared_tools_and_cleans_up_across_restart() {
    let root = tempfile::tempdir_in("/tmp").unwrap();
    let path = root.path().join("mcp.sock");
    let mut configuration = config(&root.path().join("data"));
    configuration.listen = "0.0.0.0:0".parse().unwrap();
    configuration.mcp_socket = Some(path.clone());
    let server = Server::bind(configuration.clone()).await.unwrap();
    let port = server.address().port();
    let (stop, signal) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(server.serve(async move {
        let _ = signal.await;
    }));
    let client = reqwest::Client::builder()
        .no_proxy()
        .unix_socket(path.as_path())
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let url = "http://localhost/mcp/agents";
    let request = |message: Value| {
        client
            .post(url)
            .bearer_auth(TOKEN)
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", "2025-11-25")
            .json(&message)
    };
    let initialize: Value = request(json!({"jsonrpc":"2.0","id":1,"method":"initialize",
        "params":{"protocolVersion":"2025-11-25","capabilities":{},
        "clientInfo":{"name":"unix-test","version":"1"}}}))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(initialize["result"]["serverInfo"]["name"], "ait-daemon");
    assert_eq!(
        request(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::ACCEPTED
    );
    let catalog = json!({"jsonrpc":"2.0","id":2,"method":"tools/list"});
    let unix_catalog: Value = request(catalog.clone())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let tcp_catalog: Value = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://127.0.0.1:{port}/mcp/agents"))
        .bearer_auth(TOKEN)
        .header("accept", "application/json, text/event-stream")
        .json(&catalog)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(unix_catalog, tcp_catalog);
    assert!(
        unix_catalog["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "list_workspaces")
    );
    let workspaces: Value = request(json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
        "params":{"name":"list_workspaces","arguments":{}}}))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(workspaces["result"]["isError"], false);

    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(8), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(!path.exists());
    let restarted = Server::bind(configuration).await.unwrap();
    assert!(path.exists());
    restarted.serve(async {}).await.unwrap();
    assert!(!path.exists());
}

#[tokio::test]
async fn mcp_socket_bind_failure_releases_the_instance_without_removing_other_files() {
    let root = tempfile::tempdir_in("/tmp").unwrap();
    let path = root.path().join("keep");
    std::fs::write(&path, "keep me").unwrap();
    let mut configuration = config(&root.path().join("data"));
    configuration.mcp_socket = Some(path.clone());
    assert!(Server::bind(configuration.clone()).await.is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep me");
    configuration.mcp_socket = None;
    Server::bind(configuration)
        .await
        .unwrap()
        .serve(async {})
        .await
        .unwrap();
}
