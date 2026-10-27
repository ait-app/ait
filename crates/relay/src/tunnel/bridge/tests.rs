use super::*;
use crate::diagnostics::tests::Capture;
use crate::transport::Dialer;
use secrecy::SecretString;
use tokio::net::TcpListener;
use tokio::sync::{Semaphore, oneshot};
use tokio_tungstenite::accept_async;
use tracing::instrument::WithSubscriber;

use super::super::mux;

#[tokio::test]
async fn oversized_local_message_ends_the_visit_with_a_logged_capacity_error() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let daemon = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        let _hello = socket.next().await;
        let oversized = "x".repeat(wire::MAX_BODY + 1);
        socket.send(Message::Text(oversized.into())).await.unwrap();
        std::future::pending::<()>().await;
    });
    let local = Arc::new(Local {
        visits: Arc::new(Semaphore::new(1)),
        dialer: Dialer::default(),
        url: format!("ws://{address}"),
        token: SecretString::from("local-only"),
        server_id: "server".to_owned(),
        instance_id: "instance".to_owned(),
    });
    let (center_io, host_io) = tokio::io::duplex(wire::MAX_BLOCK);
    let (requests, receiver) = mpsc::channel(1);
    let (_unused, unused) = mpsc::channel(1);
    let (incoming, mut streams) = mpsc::channel(1);
    let center = tokio::spawn(mux::drive(center_io, yamux::Mode::Client, receiver, None));
    let host = tokio::spawn(mux::drive(
        host_io,
        yamux::Mode::Server,
        unused,
        Some(incoming),
    ));
    let (send, receive) = oneshot::channel();
    requests.send(send).await.unwrap();
    let mut client = receive.await.unwrap();
    let (epoch, tunnel_id) = (Uuid::new_v4(), Uuid::new_v4());
    let open = wire::Open {
        relay_session_id: Uuid::new_v4(),
        source_node_session_id: Uuid::new_v4(),
        epoch,
        tunnel_id,
        mode: "ait-rust-single-v1".into(),
    };
    wire::write(&mut client, wire::OPEN, &serde_json::to_vec(&open).unwrap())
        .await
        .unwrap();
    let stream = streams.recv().await.unwrap();
    let capture = Capture::default();
    let visit =
        tokio::spawn(visit(stream, local, epoch, tunnel_id).with_subscriber(capture.subscriber()));
    wire::expect(&mut client, wire::ACCEPT).await.unwrap();
    wire::write(&mut client, wire::START, &[]).await.unwrap();
    let hello = br#"{ "type": "hello", "future": true }"#;
    wire::write(&mut client, wire::TEXT, hello).await.unwrap();

    let result = timeout(Duration::from_secs(5), visit)
        .await
        .unwrap()
        .unwrap();

    assert!(result.is_err());
    let log = capture.text();
    assert!(log.contains("operation=\"tunnel_local_read\""));
    assert!(log.contains("error_kind=\"capacity\""));
    assert!(log.contains(&format!("max_size={}", wire::MAX_BODY)));
    daemon.abort();
    center.abort();
    host.abort();
}
