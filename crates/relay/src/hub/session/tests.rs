use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use secrecy::SecretString;
use tokio::net::TcpListener;
use tokio::sync::{Semaphore, mpsc};
use tokio::time::timeout;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

use super::*;
use crate::transport::Dialer;

const HELLO: &str = r#"{"type":"hello","client_id":"hub-test"}"#;

fn local(address: std::net::SocketAddr) -> Arc<Local> {
    Arc::new(Local {
        visits: Arc::new(Semaphore::new(1)),
        dialer: Dialer::default(),
        url: format!("ws://{address}/v1/ws"),
        token: SecretString::from("local-token"),
        server_id: "server".to_owned(),
        instance_id: "instance".to_owned(),
    })
}

async fn next(output: &mut mpsc::Receiver<(Uuid, Output)>) -> Output {
    timeout(Duration::from_secs(3), output.recv())
        .await
        .expect("session output")
        .expect("output channel open")
        .1
}

#[tokio::test]
async fn bridges_hello_replies_and_inbound_messages_then_reports_close() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let local = local(listener.local_addr().expect("address"));
    let daemon = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.expect("accept");
        let mut socket = accept_async(tcp).await.expect("upgrade");
        let hello = socket.next().await.expect("hello").expect("hello frame");
        assert_eq!(hello.to_text().expect("text"), HELLO);
        socket
            .send(Message::Ping(Bytes::from_static(b"p")))
            .await
            .expect("ping");
        socket
            .send(Message::Text("server info".into()))
            .await
            .expect("info");
        socket
            .send(Message::Binary(Bytes::from_static(&[7])))
            .await
            .expect("binary");
        loop {
            match socket.next().await.expect("reply").expect("reply frame") {
                Message::Text(text) => {
                    socket.close(None).await.expect("close");
                    return text.to_string();
                }
                Message::Pong(_) => {}
                other => std::panic!("unexpected {other:?}"),
            }
        }
    });
    let (inbound_tx, inbound_rx) = mpsc::channel(4);
    let (output_tx, mut output_rx) = mpsc::channel(4);
    let id = Uuid::new_v4();

    tokio::spawn(run(
        local,
        id,
        Bytes::from_static(HELLO.as_bytes()),
        None,
        inbound_rx,
        output_tx,
    ));

    assert_eq!(
        next(&mut output_rx).await,
        Output::Accept(Bytes::from_static(b"server info"))
    );
    assert_eq!(
        next(&mut output_rx).await,
        Output::Message(PayloadKind::Binary, Bytes::from_static(&[7]))
    );
    inbound_tx
        .send(Message::Text("request".into()))
        .await
        .expect("deliver");
    assert_eq!(daemon.await.expect("daemon"), "request");
    assert_eq!(next(&mut output_rx).await, Output::Closed("closed"));
}

#[tokio::test]
async fn realtime_voice_controls_pcm_and_playback_confirmation_traverse_hub() {
    use serde_json::json;

    let exercise = async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local = local(listener.local_addr().unwrap());
        let (inbound_tx, inbound_rx) = mpsc::channel(4);
        let (output_tx, mut output_rx) = mpsc::channel(4);
        let task = tokio::spawn(run(
            local,
            Uuid::new_v4(),
            Bytes::from_static(HELLO.as_bytes()),
            None,
            inbound_rx,
            output_tx,
        ));
        let (tcp, _) = listener.accept().await.unwrap();
        let mut daemon = accept_async(tcp).await.unwrap();
        assert_eq!(
            daemon.next().await.unwrap().unwrap(),
            Message::Text(HELLO.into())
        );
        daemon
            .send(Message::Text("server info".into()))
            .await
            .unwrap();
        assert!(matches!(next(&mut output_rx).await, Output::Accept(_)));
        for params in [
            json!({
                "type":"request", "request_id":"start", "method":"voice.mode.set.request",
                "params":{"enabled":true,"agentId":"codex"}
            }),
            json!({
                "type":"event", "method":"voice.audio.chunk",
                "params":{"audio":"AAgACA==","format":"pcm;rate=16000","isLast":false}
            }),
            json!({"type":"event","method":"voice.audio.played","params":{"id":"audio-1"}}),
        ] {
            let frame = Message::Text(params.to_string().into());
            inbound_tx.send(frame.clone()).await.unwrap();
            assert_eq!(daemon.next().await.unwrap().unwrap(), frame);
        }
        for params in [
            json!({
                "type":"response", "request_id":"start",
                "result":{"accepted":true,"enabled":true,"agentId":"codex"}
            }),
            json!({
                "type":"event", "method":"voice.audio.output", "params":{
                    "id":"audio-1", "audio":"AAgACA==",
                    "format":"audio/pcm;rate=24000;bits=16;channels=1",
                    "isVoiceMode":true, "isLastChunk":true
                }
            }),
            json!({"type":"event","method":"voice.session.closed","params":{}}),
        ] {
            let text = params.to_string();
            daemon
                .send(Message::Text(text.clone().into()))
                .await
                .unwrap();
            assert_eq!(
                next(&mut output_rx).await,
                Output::Message(PayloadKind::Text, Bytes::from(text))
            );
        }
        drop(inbound_tx);
        assert!(matches!(
            daemon.next().await,
            Some(Ok(Message::Close(_))) | None
        ));
        task.await.unwrap();
    };
    timeout(Duration::from_secs(5), exercise).await.unwrap();
}

#[tokio::test]
async fn refuses_invalid_hello_and_unreachable_daemon() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address");
    drop(listener);
    let (output_tx, mut output_rx) = mpsc::channel(4);
    let (_inbound_tx, inbound_rx) = mpsc::channel(1);

    run(
        local(address),
        Uuid::new_v4(),
        Bytes::from_static(b"{}"),
        None,
        inbound_rx,
        output_tx.clone(),
    )
    .await;
    let (_inbound_tx, inbound_rx) = mpsc::channel(1);
    run(
        local(address),
        Uuid::new_v4(),
        Bytes::from_static(HELLO.as_bytes()),
        None,
        inbound_rx,
        output_tx,
    )
    .await;

    assert_eq!(next(&mut output_rx).await, Output::Closed("invalid_hello"));
    assert_eq!(next(&mut output_rx).await, Output::Closed("unavailable"));
}

#[test]
fn payload_kinds_map_to_business_messages() {
    assert_eq!(
        message(PayloadKind::Text, Bytes::from_static(b"hi")),
        Ok(Message::Text("hi".into()))
    );
    assert_eq!(
        message(PayloadKind::Binary, Bytes::from_static(&[0xff])),
        Ok(Message::Binary(Bytes::from_static(&[0xff])))
    );
    assert_eq!(
        message(PayloadKind::Text, Bytes::from_static(&[0xff])),
        Err("invalid_message")
    );
}

#[tokio::test]
async fn forwards_later_daemon_traffic_and_closes_when_the_link_drops_the_session() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let local = local(listener.local_addr().expect("address"));
    let daemon = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.expect("accept");
        let mut socket = accept_async(tcp).await.expect("upgrade");
        socket.next().await.expect("hello").expect("hello frame");
        socket
            .send(Message::Pong(Bytes::from_static(b"early")))
            .await
            .expect("pong");
        socket
            .send(Message::Text("server info".into()))
            .await
            .expect("info");
        socket
            .send(Message::Ping(Bytes::from_static(b"p")))
            .await
            .expect("ping");
        socket
            .send(Message::Pong(Bytes::from_static(b"q")))
            .await
            .expect("pong");
        socket
            .send(Message::Text("event".into()))
            .await
            .expect("event");
        // The session closes this socket once the link drops it.
        loop {
            match socket.next().await {
                Some(Ok(Message::Close(_))) | None => return true,
                Some(Ok(_)) => {}
                Some(Err(_)) => return false,
            }
        }
    });
    let (inbound_tx, inbound_rx) = mpsc::channel(4);
    let (output_tx, mut output_rx) = mpsc::channel(4);
    tokio::spawn(run(
        local,
        Uuid::new_v4(),
        Bytes::from_static(HELLO.as_bytes()),
        None,
        inbound_rx,
        output_tx,
    ));

    assert!(matches!(next(&mut output_rx).await, Output::Accept(_)));
    assert_eq!(
        next(&mut output_rx).await,
        Output::Message(PayloadKind::Text, Bytes::from_static(b"event"))
    );
    drop(inbound_tx);

    assert_eq!(next(&mut output_rx).await, Output::Closed("closed"));
    assert!(daemon.await.expect("daemon"), "daemon saw a clean close");
}

#[tokio::test]
async fn daemon_failures_close_the_session_as_unavailable() {
    for abrupt_after_reply in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let local = local(listener.local_addr().expect("address"));
        tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.expect("accept");
            let mut socket = accept_async(tcp).await.expect("upgrade");
            socket.next().await.expect("hello").expect("hello frame");
            if abrupt_after_reply {
                socket
                    .send(Message::Text("server info".into()))
                    .await
                    .expect("info");
            }
            // Drop the TCP connection without a WebSocket close.
            drop(socket);
        });
        let (_inbound_tx, inbound_rx) = mpsc::channel(4);
        let (output_tx, mut output_rx) = mpsc::channel(4);
        tokio::spawn(run(
            local,
            Uuid::new_v4(),
            Bytes::from_static(HELLO.as_bytes()),
            None,
            inbound_rx,
            output_tx,
        ));
        if abrupt_after_reply {
            assert!(matches!(next(&mut output_rx).await, Output::Accept(_)));
        }
        assert_eq!(next(&mut output_rx).await, Output::Closed("unavailable"));
    }
}

#[tokio::test]
async fn a_session_whose_link_is_gone_stops_after_the_handshake() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let local = local(listener.local_addr().expect("address"));
    let daemon = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.expect("accept");
        let mut socket = accept_async(tcp).await.expect("upgrade");
        socket.next().await.expect("hello").expect("hello frame");
        socket
            .send(Message::Text("server info".into()))
            .await
            .expect("info");
        while let Some(Ok(message)) = socket.next().await {
            if matches!(message, Message::Close(_)) {
                break;
            }
        }
    });
    let (_inbound_tx, inbound_rx) = mpsc::channel(4);
    let (output_tx, output_rx) = mpsc::channel(4);
    drop(output_rx);

    timeout(
        Duration::from_secs(3),
        run(
            local,
            Uuid::new_v4(),
            Bytes::from_static(HELLO.as_bytes()),
            None,
            inbound_rx,
            output_tx,
        ),
    )
    .await
    .expect("the bridge returns once nobody reads its output");
    timeout(Duration::from_secs(3), daemon)
        .await
        .expect("daemon socket released")
        .expect("daemon");
}

#[tokio::test]
async fn guest_sessions_dial_with_the_visitor_and_report_refusal_as_revoked() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let local = local(listener.local_addr().expect("address"));
    let visitor = model::share::Visitor {
        user_id: Uuid::new_v4(),
        display_name: Some("Bob".to_owned()),
        username: Some("bob".to_owned()),
        group_id: Uuid::new_v4(),
        share_id: Uuid::new_v4(),
        resource_kind: model::share::ResourceKind::Node,
        resource_key: String::new(),
        permission: "operate".to_owned(),
    };
    let expected = visitor.clone();
    let daemon = tokio::spawn(accept_guest(listener));
    let (_inbound_tx, inbound_rx) = mpsc::channel(1);
    let (output_tx, mut output_rx) = mpsc::channel(4);

    run(
        local.clone(),
        Uuid::new_v4(),
        Bytes::from_static(HELLO.as_bytes()),
        Some(visitor.encode()),
        inbound_rx,
        output_tx.clone(),
    )
    .await;
    assert_eq!(daemon.await.expect("daemon"), Some(expected));
    assert_eq!(next(&mut output_rx).await, Output::Closed("unavailable"));

    // A daemon that refuses the guest upgrade ends the session as revoked.
    let (_inbound_tx, inbound_rx) = mpsc::channel(1);
    run(
        local,
        Uuid::new_v4(),
        Bytes::from_static(HELLO.as_bytes()),
        Some(visitor.encode()),
        inbound_rx,
        output_tx,
    )
    .await;
    assert_eq!(next(&mut output_rx).await, Output::Closed("share_revoked"));
}

/// Accept one dial, record its guest header and drop the connection.
#[expect(
    clippy::result_large_err,
    reason = "tungstenite's handshake callback fixes the error type"
)]
async fn accept_guest(listener: TcpListener) -> Option<model::share::Visitor> {
    let (tcp, _) = listener.accept().await.expect("accept");
    let mut seen = None;
    let socket = accept_hdr_async(tcp, |request: &Request, response: Response| {
        seen = request
            .headers()
            .get(model::share::GUEST_HEADER)
            .and_then(|value| value.to_str().ok())
            .and_then(model::share::Visitor::decode);
        Ok(response)
    })
    .await
    .expect("upgrade");
    drop(socket);
    seen
}
