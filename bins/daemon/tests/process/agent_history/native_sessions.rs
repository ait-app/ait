//! A native session-list wait must not hold later requests on either WebSocket transport.

use std::path::PathBuf;
use std::time::Duration;

use futures_util::SinkExt;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

use super::super::native::NativeFixture;
use super::super::transport::{Socket, connect, receive};
use super::{ready, start_with_path, terminate};

struct ListingGate(PathBuf);

impl ListingGate {
    fn new(fixture: &NativeFixture) -> Self {
        std::fs::write(fixture.cwd.join("behavior"), "delayed-listing").unwrap();
        Self(fixture.cwd.clone())
    }

    async fn entered(&self) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while !self.0.join("listing-entered").exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("native enumeration reached its gate");
    }

    fn release(&self) {
        std::fs::write(self.0.join("release-listing"), "").unwrap();
    }
}

impl Drop for ListingGate {
    fn drop(&mut self) {
        let _ = std::fs::write(self.0.join("release-listing"), "");
    }
}

async fn send_listing(socket: &mut Socket, id: &str, params: Value) {
    socket
        .send(Message::Text(
            json!({"type":"request","request_id":id,
                "method":"provider.sessions.recent.list.request","params":params})
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
}

#[tokio::test]
async fn native_session_listing_does_not_block_legacy_physical_socket() {
    assert_responsive(false).await;
}

#[tokio::test]
async fn native_session_listing_does_not_block_single_connection_transport() {
    assert_responsive(true).await;
}

async fn assert_responsive(single: bool) {
    let fixture = NativeFixture::new();
    let gate = ListingGate::new(&fixture);
    let state = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut methods = vec!["provider.sessions.recent.list.request"];
    if single {
        methods.push("connection.single.v1");
    }
    let mut socket = connect(&address, &methods).await;

    send_listing(
        &mut socket,
        "slow",
        json!({"providers":["codex"],"cwd":fixture.cwd,"limit":15}),
    )
    .await;
    gate.entered().await;
    send_listing(&mut socket, "fast", json!({"providers":[],"limit":15})).await;
    let fast = tokio::time::timeout(Duration::from_secs(2), receive(&mut socket))
        .await
        .expect("another listing must complete on the same socket before the gate opens");
    assert_eq!(fast["request_id"], "fast", "{fast}");
    assert_eq!(fast["result"]["entries"], json!([]));

    gate.release();
    let slow = receive(&mut socket).await;
    assert_eq!(slow["request_id"], "slow", "{slow}");
    assert_eq!(slow["result"]["entries"], json!([]));
    socket.close(None).await.unwrap();
    terminate(&mut process).await;
}
