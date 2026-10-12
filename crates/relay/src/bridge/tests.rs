use super::*;
use crate::diagnostics::tests::Capture;
use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, frame::coding::CloseCode};
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn close_frame_logs_side_session_and_code_without_reason_text() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        socket
            .send(Message::Close(Some(CloseFrame {
                code: CloseCode::Policy,
                reason: "private-close-text".into(),
            })))
            .await
            .unwrap();
        let _ = socket.next().await;
    });
    let socket = connect(&format!("ws://{address}"), "private-ticket", MAX_MESSAGE)
        .await
        .unwrap();
    let (peer, _peer_rx) = mpsc::channel(2);
    let (_incoming_tx, incoming) = mpsc::channel(2);
    let cancel = CancellationToken::new();
    let capture = Capture::default();
    let session = Uuid::new_v4();
    timeout(Duration::from_secs(3), async {
        endpoint(socket, peer, incoming, cancel.clone())
            .instrument(tracing::info_span!("relay_session", relay_session_id = %session, side = "local_daemon"))
            .await;
    }.with_subscriber(capture.subscriber())).await.unwrap();
    server.await.unwrap();
    assert!(cancel.is_cancelled());
    let log = capture.text();
    assert!(log.contains(&session.to_string()));
    assert!(log.contains("local_daemon"));
    assert!(log.contains("close_code=1008"));
    assert!(log.contains("close_frame"));
    assert!(log.contains("elapsed_ms="));
    assert!(!log.contains("private-"));
}

#[tokio::test]
async fn blocked_forwarding_reports_queue_timeout_and_cancels_pair() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        socket
            .send(Message::Text("private-payload".into()))
            .await
            .unwrap();
        let _ = socket.next().await;
    });
    let socket = connect(&format!("ws://{address}"), "private-ticket", MAX_MESSAGE)
        .await
        .unwrap();
    let (peer, _peer_rx) = mpsc::channel(1);
    peer.send(Message::Text("full".into())).await.unwrap();
    let (_incoming_tx, incoming) = mpsc::channel(2);
    let cancel = CancellationToken::new();
    let capture = Capture::default();
    timeout(
        Duration::from_secs(8),
        endpoint(socket, peer, incoming, cancel.clone()).with_subscriber(capture.subscriber()),
    )
    .await
    .unwrap();
    server.await.unwrap();
    assert!(cancel.is_cancelled());
    let log = capture.text();
    assert!(log.contains("peer_queue_timeout"));
    assert!(log.contains("timeout_ms=5000"));
    assert!(!log.contains("private-"));
}

#[tokio::test]
async fn abrupt_socket_loss_reports_read_error_and_cancels_pair() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let socket = accept_async(tcp).await.unwrap();
        drop(socket);
    });
    let socket = connect(&format!("ws://{address}"), "private-ticket", MAX_MESSAGE)
        .await
        .unwrap();
    let (peer, _peer_rx) = mpsc::channel(2);
    let (_incoming_tx, incoming) = mpsc::channel(2);
    let cancel = CancellationToken::new();
    let capture = Capture::default();
    timeout(
        Duration::from_secs(3),
        endpoint(socket, peer, incoming, cancel.clone()).with_subscriber(capture.subscriber()),
    )
    .await
    .unwrap();
    server.await.unwrap();
    assert!(cancel.is_cancelled());
    let log = capture.text();
    assert!(log.contains("operation=\"read\""));
    assert!(log.contains("error_kind="));
    assert!(!log.contains("private-"));
}
