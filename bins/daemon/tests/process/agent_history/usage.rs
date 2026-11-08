//! A pending quota read must not hold chat loading on the same physical connection.

use std::path::PathBuf;
use std::time::Duration;

use futures_util::SinkExt;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

use super::super::native::NativeFixture;
use super::super::transport::{Socket, connect, receive, request};
use super::{METHODS, ready, start_with_path, terminate};

struct UsageGate(PathBuf);

impl UsageGate {
    fn new(fixture: &NativeFixture) -> Self {
        std::fs::write(fixture.cwd.join("hold-usage"), "").unwrap();
        Self(fixture.cwd.clone())
    }

    async fn entered(&self) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !self.0.join("usage-entered").exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("offline quota query entered its gate");
    }

    fn release(&self) {
        std::fs::write(self.0.join("release-usage"), "").unwrap();
    }
}

impl Drop for UsageGate {
    fn drop(&mut self) {
        let _ = std::fs::write(self.0.join("release-usage"), "");
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
async fn slow_usage_does_not_block_single_connection_chat_loading() {
    Box::pin(assert_responsive(true)).await;
}

#[tokio::test]
async fn slow_usage_does_not_block_legacy_connection_chat_loading() {
    Box::pin(assert_responsive(false)).await;
}

async fn assert_responsive(single: bool) {
    let fixture = NativeFixture::new();
    std::fs::write(
        fixture.root.path().join("codex.cwd"),
        fixture.cwd.to_str().unwrap(),
    )
    .unwrap();
    let state = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut methods = METHODS.to_vec();
    methods.extend(["connection.ping", "provider.usage.list.request"]);
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
    let gate = UsageGate::new(&fixture);
    send(
        &mut socket,
        "slow-usage",
        "provider.usage.list.request",
        json!({"providerId":"codex","forceRefresh":true}),
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
            .expect("pending usage must not block chat loading on this connection");
        assert_eq!(response["request_id"], request_id, "{response}");
        assert_eq!(response["type"], "response", "{response}");
    }
    gate.release();
    let response = receive(&mut socket).await;
    assert_eq!(response["request_id"], "slow-usage");
    assert_eq!(response["result"]["providers"][0]["status"], "available");
    terminate(&mut process).await;
}
