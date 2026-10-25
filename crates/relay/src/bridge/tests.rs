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
    let socket = crate::transport::Dialer::default()
        .connect(&format!("ws://{address}"), "private-ticket", MAX_MESSAGE)
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
    let socket = crate::transport::Dialer::default()
        .connect(&format!("ws://{address}"), "private-ticket", MAX_MESSAGE)
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
    let socket = crate::transport::Dialer::default()
        .connect(&format!("ws://{address}"), "private-ticket", MAX_MESSAGE)
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

#[tokio::test]
async fn data_setup_times_out_pairing_and_client_hello_without_contacting_local_daemon() {
    for paired in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let session = Uuid::new_v4();
        let local = Arc::new(Local {
            visits: Arc::new(tokio::sync::Semaphore::new(16)),
            dialer: crate::transport::Dialer::default(),
            url: "ws://127.0.0.1:1".into(),
            token: "local-fixture".into(),
            server_id: "server".into(),
            instance_id: "instance".into(),
        });
        let task = tokio::spawn(data(
            format!("ws://{address}"),
            "remote-fixture".into(),
            local,
            session,
        ));
        let (tcp, _) = listener.accept().await.unwrap();
        let mut remote = accept_async(tcp).await.unwrap();
        if paired {
            remote
                .send(Message::Text(
                    serde_json::json!({"type":"relay.ready","relay_session_id":session})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
        }
        remote.send(Message::Ping(vec![1].into())).await.unwrap();
        assert_eq!(
            remote.next().await.unwrap().unwrap(),
            Message::Pong(vec![1].into())
        );
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(if paired { 11 } else { 31 })).await;
        assert!(matches!(task.await.unwrap(), Err(Error::Transport)));
        tokio::time::resume();
    }
}

#[tokio::test]
async fn closed_forwarding_queue_cancels_both_endpoints_and_answers_hop_pings() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        socket.send(Message::Ping(vec![9].into())).await.unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap(),
            Message::Pong(vec![9].into())
        );
        socket.send(Message::Pong(vec![8].into())).await.unwrap();
        socket.send(Message::Text("fixture".into())).await.unwrap();
        let _ = socket.next().await;
    });
    let socket = crate::transport::Dialer::default()
        .connect(&format!("ws://{address}"), "fixture", MAX_MESSAGE)
        .await
        .unwrap();
    let (peer, peer_rx) = mpsc::channel(2);
    drop(peer_rx);
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
    assert!(capture.text().contains("peer_queue_closed"));
}

#[tokio::test]
async fn blocked_socket_writes_bound_ping_backlog_and_close_both_endpoints() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        // A peek fences the start of the large outbound write without draining it.
        socket.get_ref().peek(&mut [0; 1]).await.unwrap();
        for _ in 0..3 {
            socket.send(Message::Ping(vec![1].into())).await.unwrap();
        }
        std::future::pending::<()>().await;
    });
    let socket = crate::transport::Dialer::default()
        .connect(&format!("ws://{address}"), "fixture", MAX_MESSAGE)
        .await
        .unwrap();
    let (peer, _peer_rx) = mpsc::channel(2);
    let (incoming_tx, incoming) = mpsc::channel(2);
    incoming_tx
        .send(Message::Binary(vec![0; 32 * 1024 * 1024].into()))
        .await
        .unwrap();
    let cancel = CancellationToken::new();
    let capture = Capture::default();

    timeout(
        Duration::from_secs(9),
        endpoint(socket, peer, incoming, cancel.clone()).with_subscriber(capture.subscriber()),
    )
    .await
    .unwrap();

    assert!(cancel.is_cancelled());
    assert!(capture.text().contains("pong_queue_full"));
    assert!(capture.text().contains("write_timeout"));
    server.abort();
}

#[tokio::test]
async fn closed_incoming_queue_stops_the_writer_without_waiting_for_a_peer_frame() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        let _ = socket.next().await;
    });
    let socket = crate::transport::Dialer::default()
        .connect(&format!("ws://{address}"), "fixture", MAX_MESSAGE)
        .await
        .unwrap();
    let (peer, _peer_rx) = mpsc::channel(2);
    let (incoming_tx, incoming) = mpsc::channel(2);
    drop(incoming_tx);
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
    assert!(capture.text().contains("incoming_queue_closed"));
}

#[tokio::test]
async fn closed_socket_write_stops_owned_forwarding_and_reports_a_transport_category() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        let _ = socket.next().await;
    });
    let mut socket = crate::transport::Dialer::default()
        .connect(&format!("ws://{address}"), "fixture", MAX_MESSAGE)
        .await
        .unwrap();
    socket.close(None).await.unwrap();
    let (sink, _stream) = socket.split();
    let (incoming_tx, incoming) = mpsc::channel(2);
    incoming_tx
        .send(Message::Text("private fixture".into()))
        .await
        .unwrap();
    let (_pong_tx, pong) = mpsc::channel(1);
    let cancel = CancellationToken::new();
    let capture = Capture::default();

    timeout(
        Duration::from_secs(3),
        write(sink, incoming, pong, &cancel).with_subscriber(capture.subscriber()),
    )
    .await
    .unwrap();

    assert!(cancel.is_cancelled());
    assert!(capture.text().contains("operation=\"write\""));
    assert!(!capture.text().contains("private fixture"));
    server.await.unwrap();
}

#[tokio::test]
async fn realtime_voice_controls_pcm_and_close_traverse_the_online_bridge() {
    timeout(Duration::from_secs(5), async {
        let remote_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let session = Uuid::new_v4();
        let local = Arc::new(Local {
            visits: Arc::new(tokio::sync::Semaphore::new(16)),
            dialer: crate::transport::Dialer::default(),
            url: format!("ws://{}", local_listener.local_addr().unwrap()),
            token: "fixture".into(), server_id: "server".into(), instance_id: "instance".into(),
        });
        let bridge = tokio::spawn(data(
            format!("ws://{}", remote_listener.local_addr().unwrap()), "fixture".into(), local, session,
        ));
        let (tcp, _) = remote_listener.accept().await.unwrap();
        let mut client = accept_async(tcp).await.unwrap();
        client.send(Message::Text(serde_json::json!({"type":"relay.ready","relay_session_id":session}).to_string().into())).await.unwrap();
        let hello = Message::Text(serde_json::json!({"type":"hello","client_id":"voice-client"}).to_string().into());
        client.send(hello.clone()).await.unwrap();
        let (tcp, _) = local_listener.accept().await.unwrap();
        let mut daemon = accept_async(tcp).await.unwrap();
        assert_eq!(daemon.next().await.unwrap().unwrap(), hello);
        for params in [
            serde_json::json!({"type":"request","request_id":"start","method":"voice.mode.set.request","params":{"enabled":true,"agentId":"codex"}}),
            serde_json::json!({"type":"event","method":"voice.audio.chunk","params":{"audio":"AAgACA==","format":"pcm;rate=16000","isLast":false}}),
            serde_json::json!({"type":"event","method":"voice.audio.played","params":{"id":"audio-1"}}),
        ] {
            let frame = Message::Text(params.to_string().into());
            client.send(frame.clone()).await.unwrap();
            assert_eq!(daemon.next().await.unwrap().unwrap(), frame);
        }
        for params in [
            serde_json::json!({"type":"response","request_id":"start","result":{"accepted":true,"enabled":true,"agentId":"codex"}}),
            serde_json::json!({"type":"event","method":"voice.audio.output","params":{"id":"audio-1","audio":"AAgACA==","format":"audio/pcm;rate=24000;bits=16;channels=1","isVoiceMode":true,"isLastChunk":true}}),
            serde_json::json!({"type":"event","method":"voice.session.closed","params":{}}),
        ] {
            let frame = Message::Text(params.to_string().into());
            daemon.send(frame.clone()).await.unwrap();
            assert_eq!(client.next().await.unwrap().unwrap(), frame);
        }
        client.close(None).await.unwrap();
        assert!(matches!(daemon.next().await, Some(Ok(Message::Close(_))) | None));
        bridge.await.unwrap().unwrap();
    }).await.unwrap();
}
