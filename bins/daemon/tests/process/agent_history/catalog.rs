//! Physical-socket discovery cannot hold timeline requests or liveness behind native startup.

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::SinkExt;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

use super::super::native::NativeFixture;
use super::super::transport::{Socket, connect, receive, request};
use super::{METHODS, ready, start_with_path, terminate};

struct DiscoveryGate(PathBuf);

impl DiscoveryGate {
    fn new(cwd: &Path) -> Self {
        std::fs::write(cwd.join("behavior"), "delayed-discovery").unwrap();
        Self(cwd.to_owned())
    }

    async fn entered(&self) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while !self.0.join("discovery-entered").exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("offline provider reached the discovery gate");
    }

    fn release(&self) {
        std::fs::write(self.0.join("release-discovery"), "").unwrap();
    }

    fn reset(&self) {
        std::fs::remove_file(self.0.join("discovery-entered")).unwrap();
        std::fs::remove_file(self.0.join("release-discovery")).unwrap();
    }
}

impl Drop for DiscoveryGate {
    fn drop(&mut self) {
        let _ = std::fs::write(self.0.join("release-discovery"), "");
    }
}

async fn send(socket: &mut Socket, id: &str, method: &str, params: Value) {
    socket
        .send(Message::Text(
            json!({"type":"request","request_id":id,"method":method,"params":params})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
}

#[tokio::test]
async fn slow_catalog_does_not_block_legacy_physical_socket() {
    Box::pin(assert_responsive(false)).await;
}

#[tokio::test]
async fn slow_catalog_does_not_block_single_connection_transport() {
    Box::pin(assert_responsive(true)).await;
}

async fn assert_responsive(single: bool) {
    let fixture = NativeFixture::new();
    let state = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut methods = METHODS.to_vec();
    methods.push("connection.ping");
    if single {
        methods.push("connection.single.v1");
    }
    let mut socket = connect(&address, &methods).await;
    let workspace = request(
        &mut socket,
        "workspace.open.request",
        json!({"cwd":fixture.cwd}),
    )
    .await;
    assert_eq!(workspace["type"], "response");
    let created = request(
        &mut socket,
        "agent.create.request",
        json!({"config":{"provider":"codex","cwd":fixture.cwd}}),
    )
    .await;
    let id = created["result"]["agentId"].as_str().unwrap();
    let gate = DiscoveryGate::new(&fixture.cwd);
    send(
        &mut socket,
        "slow-catalog",
        "provider.models.list.request",
        json!({"provider":"codex","cwd":fixture.cwd}),
    )
    .await;
    gate.entered().await;
    for (request_id, method, params) in [
        ("alive", "connection.ping", json!({"nonce":"alive"})),
        ("agent", "agent.get.request", json!({"agentId":id})),
        (
            "history",
            "agent.timeline.get.request",
            json!({"agentId":id}),
        ),
        (
            "observe",
            "agent.timeline.set_subscription.request",
            json!({"agentIds":[id]}),
        ),
    ] {
        send(&mut socket, request_id, method, params).await;
        let response = tokio::time::timeout(Duration::from_secs(2), receive(&mut socket))
            .await
            .expect("catalog discovery must not block this physical socket");
        assert_eq!(response["request_id"], request_id, "{response}");
        assert_eq!(response["type"], "response", "{response}");
    }
    gate.release();
    let response = receive(&mut socket).await;
    assert_eq!(response["request_id"], "slow-catalog");
    assert!(response["result"]["models"].is_array());
    let response = request(
        &mut socket,
        "provider.snapshot.get.request",
        json!({"cwd":fixture.cwd}),
    )
    .await;
    assert!(
        response["result"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["provider"] == "codex" && entry["status"] == "ready")
    );
    Box::pin(assert_disconnected_refresh(
        &address,
        &methods,
        &fixture.cwd,
        &gate,
        socket,
    ))
    .await;
    terminate(&mut process).await;
}

async fn assert_disconnected_refresh(
    address: &str,
    methods: &[&str],
    cwd: &Path,
    gate: &DiscoveryGate,
    mut socket: Socket,
) {
    gate.reset();
    send(
        &mut socket,
        "detached-refresh",
        "provider.snapshot.refresh.request",
        json!({"cwd":cwd}),
    )
    .await;
    gate.entered().await;
    socket.close(None).await.unwrap();
    let mut surviving = connect(address, methods).await;
    assert_eq!(
        request(
            &mut surviving,
            "connection.ping",
            json!({"nonce":"survivor"})
        )
        .await["result"]["nonce"],
        "survivor"
    );
    gate.release();
    let models = request(
        &mut surviving,
        "provider.models.list.request",
        json!({"provider":"codex","cwd":cwd}),
    )
    .await;
    assert_eq!(models["type"], "response");
    let snapshot = request(
        &mut surviving,
        "provider.snapshot.get.request",
        json!({"cwd":cwd}),
    )
    .await;
    assert_eq!(snapshot["type"], "response");
    let count = std::fs::read_to_string(cwd.join("native-requests.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|request| request["method"] == "model/list")
        .count();
    assert_eq!(
        count, 2,
        "disconnected refresh completes and later reads reuse its cache"
    );
}
