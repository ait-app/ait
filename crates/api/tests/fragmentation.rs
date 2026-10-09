//! Fragmentation must not bypass the cumulative input message budget.

// Cargo passes the complete package dependency set to this integration test.
use browser as _;
use chrono as _;
use domain as _;
use filesystem as _;
use metadata as _;
use model as _;
use persistence as _;
use provider as _;
use relay as _;
use reqwest as _;
use schedule as _;
use secrecy as _;
use serde as _;
use subtle as _;
use tempfile as _;
use terminal as _;
use thiserror as _;
use tokio_util as _;
use tower_http as _;
use tracing as _;
use uuid as _;
use voice as _;

use api::Api;
use futures_util::{SinkExt, StreamExt};
use model::server::MAX_MESSAGE_BYTES;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::frame::Frame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::{Data, OpCode};
use tokio_tungstenite::tungstenite::{Error, Message};

#[tokio::test]
async fn fragmented_message_cannot_exceed_total_budget() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let token = "offline-fragment-test-token-with-32-characters";
    let api = Api::new(
        address,
        "stable".to_owned(),
        "instance".to_owned(),
        token.into(),
        api::Services::default(),
    )
    .unwrap();
    let shutdown = api.clone();
    let router = api.router();
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move { shutdown.wait_draining().await })
            .await
            .unwrap();
    });
    let mut request = format!("ws://{address}/v1/ws")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("authorization", format!("Bearer {token}").parse().unwrap());
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    // Otherwise valid hello with an optional future field: without the cumulative limit
    // the server would accept it. Each individual frame remains below the frame limit.
    let hello = serde_json::json!({"type":"hello", "client_id":"fragmented", "protocol":{"major":1,"min_minor":0,"max_minor":0}, "future": "x".repeat(MAX_MESSAGE_BYTES)}).to_string();
    let (first, second) = hello.as_bytes().split_at(hello.len() / 2);
    socket
        .send(Message::Frame(Frame::message(
            first.to_vec(),
            OpCode::Data(Data::Text),
            false,
        )))
        .await
        .unwrap();
    let sent = socket
        .send(Message::Frame(Frame::message(
            second.to_vec(),
            OpCode::Data(Data::Continue),
            true,
        )))
        .await;
    if let Err(error) = sent {
        assert!(matches!(error, Error::Io(error)
            if matches!(error.kind(), std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted)));
    } else {
        let result = tokio::time::timeout(std::time::Duration::from_secs(3), socket.next())
            .await
            .unwrap();
        match result {
            Some(Ok(Message::Text(text))) => {
                let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                assert_eq!(value["code"], "invalid_message");
            }
            None | Some(Err(Error::Protocol(_) | Error::Io(_)) | Ok(Message::Close(_))) => {}
            unexpected => panic!("unexpected oversized message response: {unexpected:?}"),
        }
    }
    api.begin_shutdown();
    task.await.unwrap();
    api.wait_closed().await;
}
