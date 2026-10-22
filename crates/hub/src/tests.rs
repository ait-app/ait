use super::*;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::{net::TcpListener, time::timeout};
use tokio_tungstenite::{
    accept_async, accept_hdr_async,
    tungstenite::{Bytes, Message},
};

type Socket = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;
const HELLO: &str = r#"{ "type":"hello", "client_id":"test", "future":true }"#;

fn frame(header: &Value, payload: &[u8]) -> Message {
    let header = header.to_string();
    let mut bytes = u16::try_from(header.len()).unwrap().to_be_bytes().to_vec();
    bytes.extend_from_slice(header.as_bytes());
    bytes.extend_from_slice(payload);
    Message::Binary(bytes.into())
}

async fn next(socket: &mut Socket) -> (Value, Bytes) {
    loop {
        let message = timeout(Duration::from_secs(5), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        match message {
            Message::Binary(bytes) => {
                let n = usize::from(u16::from_be_bytes([bytes[0], bytes[1]]));
                return (
                    serde_json::from_slice(&bytes[2..2 + n]).unwrap(),
                    bytes.slice(2 + n..),
                );
            }
            Message::Ping(bytes) => socket.send(Message::Pong(bytes)).await.unwrap(),
            other => panic!("unexpected message: {other:?}"),
        }
    }
}

async fn routed(socket: &mut Socket) -> (Value, Bytes) {
    loop {
        let (header, body) = next(socket).await;
        if header["t"] != "ack" {
            return (header, body);
        }
    }
}

async fn welcome(socket: &mut Socket, link: Uuid, node: Uuid, resumed: bool, recv: u64) {
    socket.send(frame(&json!({"t":"welcome","v":1,"link":link,"node_id":node,
        "resumed":resumed,"recv":recv,"resume_token":if resumed {Value::Null} else {json!("b".repeat(64))},
        "grace_seconds":120,"ping_seconds":20,"max_payload":1_048_576}),&[])).await.unwrap();
}

#[expect(
    clippy::result_large_err,
    reason = "Tungstenite handshake callback returns an unboxed HTTP response"
)]
async fn center_socket(listener: &TcpListener, expected: &str) -> Socket {
    let (tcp, _) = timeout(Duration::from_secs(5), listener.accept())
        .await
        .unwrap()
        .unwrap();
    accept_hdr_async(
        tcp,
        |request: &tokio_tungstenite::tungstenite::handshake::server::Request, response| {
            assert_eq!(request.uri().path(), "/api/v1/hub");
            assert_eq!(request.headers()["authorization"], expected);
            Ok(response)
        },
    )
    .await
    .unwrap()
}

async fn local_echo(listener: TcpListener) {
    let (tcp, _) = listener.accept().await.unwrap();
    echo_connection(tcp).await;
}

async fn echo_connection(tcp: tokio::net::TcpStream) {
    let mut socket = accept_async(tcp).await.unwrap();
    assert_eq!(
        socket.next().await.unwrap().unwrap().to_text().unwrap(),
        HELLO
    );
    socket
        .send(Message::Text(
            json!({"type":"server_info","info":{"server_id":"stable"}})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    while let Some(Ok(message)) = socket.next().await {
        match message {
            Message::Text(_) | Message::Binary(_) => {
                if socket.send(message).await.is_err() {
                    break;
                }
            }
            Message::Close(_) => break,
            Message::Ping(bytes) => {
                let _ = socket.send(Message::Pong(bytes)).await;
            }
            _ => {}
        }
    }
}

async fn start(local: &TcpListener, center: &TcpListener) -> Connector {
    let connector = Connector::new(
        local.local_addr().unwrap(),
        "local-secret".into(),
        "stable".into(),
        "instance".into(),
    );
    connector
        .start(ControlGrant {
            center_url: format!("http://{}/api", center.local_addr().unwrap()),
            hub_ticket: "a".repeat(64).into(),
            node_session_id: Uuid::new_v4(),
        })
        .await
        .unwrap();
    connector
}

#[tokio::test]
async fn serve_keeps_local_socket_and_replays_only_unacknowledged_frames_after_reconnect() {
    let center = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let connector = start(&local, &center).await;
    let echo = tokio::spawn(local_echo(local));
    let mut socket = center_socket(&center, &format!("Bearer {}", "a".repeat(64))).await;
    assert_eq!(
        next(&mut socket).await.0,
        json!({"t":"hello","v":1,"resume":null})
    );
    let link = Uuid::new_v4();
    let node = Uuid::new_v4();
    welcome(&mut socket, link, node, false, 0).await;
    let s = Uuid::new_v4();
    socket
        .send(frame(
            &json!({"t":"open","seq":1,"s":s,"from":Uuid::new_v4()}),
            HELLO.as_bytes(),
        ))
        .await
        .unwrap();
    let (accepted, info) = routed(&mut socket).await;
    assert_eq!(accepted["t"], "accept");
    assert_eq!(
        serde_json::from_slice::<Value>(&info).unwrap()["type"],
        "server_info"
    );
    socket
        .send(frame(&json!({"t":"ack","a":accepted["seq"]}), &[]))
        .await
        .unwrap();
    socket
        .send(frame(
            &json!({"t":"msg","seq":2,"s":s,"pt":"binary"}),
            &[0, 255, 1],
        ))
        .await
        .unwrap();
    let original = routed(&mut socket).await;
    assert_eq!(original.1.as_ref(), &[0, 255, 1]);
    drop(socket);
    let mut resumed = center_socket(&center, &format!("Bearer {}", "b".repeat(64))).await;
    let hello = next(&mut resumed).await.0;
    assert_eq!(hello["resume"], json!({"link":link,"recv":2}));
    welcome(
        &mut resumed,
        link,
        node,
        true,
        accepted["seq"].as_u64().unwrap(),
    )
    .await;
    assert_eq!(routed(&mut resumed).await, original);
    // A replayed center message must not reach the local daemon twice.
    resumed
        .send(frame(
            &json!({"t":"msg","seq":2,"s":s,"pt":"binary"}),
            b"duplicate",
        ))
        .await
        .unwrap();
    resumed
        .send(frame(
            &json!({"t":"msg","seq":3,"s":s,"pt":"text"}),
            b"after",
        ))
        .await
        .unwrap();
    let reply = routed(&mut resumed).await;
    assert_eq!(reply.1.as_ref(), b"after");
    assert_eq!(reply.0["s"], s.to_string());
    assert_eq!(connector.status().await.link, Some(link));
    assert!(
        !serde_json::to_string(&connector.status().await)
            .unwrap()
            .contains(&"b".repeat(64))
    );
    connector.stop().await;
    timeout(Duration::from_secs(5), echo)
        .await
        .unwrap()
        .unwrap();
    assert!(!connector.status().await.online);
}

#[tokio::test]
async fn invalid_grants_and_shutdown_never_open_remote_connections() {
    let connector = Connector::new(
        "127.0.0.1:9".parse().unwrap(),
        "secret".into(),
        "stable".into(),
        "instance".into(),
    );
    for center in [
        "http://remote.test",
        "https://user:secret@test",
        "https://test?ticket=secret",
    ] {
        assert!(
            connector
                .start(ControlGrant {
                    center_url: center.into(),
                    hub_ticket: "a".repeat(64).into(),
                    node_session_id: Uuid::new_v4()
                })
                .await
                .is_err()
        );
    }
    assert!(
        connector
            .start(ControlGrant {
                center_url: "https://test".into(),
                hub_ticket: "short".into(),
                node_session_id: Uuid::new_v4()
            })
            .await
            .is_err()
    );
    connector.stop().await;
    connector.begin_shutdown();
    assert!(
        connector
            .start(ControlGrant {
                center_url: "https://test".into(),
                hub_ticket: "a".repeat(64).into(),
                node_session_id: Uuid::new_v4()
            })
            .await
            .is_err()
    );
}

#[tokio::test]
async fn explicit_revocation_closes_sessions_without_attempting_resume() {
    let center = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let connector = start(&local, &center).await;
    let mut socket = center_socket(&center, &format!("Bearer {}", "a".repeat(64))).await;
    next(&mut socket).await;
    welcome(&mut socket, Uuid::new_v4(), Uuid::new_v4(), false, 0).await;
    socket
        .send(frame(&json!({"t":"error","code":"replaced"}), &[]))
        .await
        .unwrap();
    timeout(Duration::from_secs(3), async {
        loop {
            if connector.status().await.error == Some("replaced") {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(
        timeout(Duration::from_millis(800), center.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn multiple_sessions_and_bad_open_are_isolated_on_one_connection() {
    let center = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let connector = start(&local, &center).await;
    let echo = tokio::spawn(async move {
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..2 {
            let (tcp, _) = local.accept().await.unwrap();
            tasks.spawn(echo_connection(tcp));
        }
        while let Some(result) = tasks.join_next().await {
            result.unwrap();
        }
    });
    let mut socket = center_socket(&center, &format!("Bearer {}", "a".repeat(64))).await;
    next(&mut socket).await;
    welcome(&mut socket, Uuid::new_v4(), Uuid::new_v4(), false, 0).await;
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    for (seq, s) in [(1, a), (2, b)] {
        socket
            .send(frame(
                &json!({"t":"open","seq":seq,"s":s,"from":Uuid::new_v4()}),
                HELLO.as_bytes(),
            ))
            .await
            .unwrap();
        assert_eq!(routed(&mut socket).await.0["s"], s.to_string());
    }
    let bad = Uuid::new_v4();
    socket
        .send(frame(
            &json!({"t":"open","seq":3,"s":bad,"from":Uuid::new_v4()}),
            b"invalid",
        ))
        .await
        .unwrap();
    let refusal = routed(&mut socket).await.0;
    assert_eq!(refusal["t"], "close");
    assert_eq!(refusal["code"], "invalid_hello");
    socket
        .send(frame(
            &json!({"t":"close","seq":4,"s":a,"code":"closed"}),
            &[],
        ))
        .await
        .unwrap();
    socket
        .send(frame(
            &json!({"t":"msg","seq":5,"s":b,"pt":"text"}),
            b"still-working",
        ))
        .await
        .unwrap();
    let reply = routed(&mut socket).await;
    assert_eq!(reply.0["s"], b.to_string());
    assert_eq!(reply.1.as_ref(), b"still-working");
    connector.stop().await;
    timeout(Duration::from_secs(5), echo)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn session_limit_does_not_close_the_link() {
    let center = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let connector = start(&local, &center).await;
    let mut socket = center_socket(&center, &format!("Bearer {}", "a".repeat(64))).await;
    next(&mut socket).await;
    welcome(&mut socket, Uuid::new_v4(), Uuid::new_v4(), false, 0).await;
    // Do not upgrade the local sockets: all 16 setup tasks hold their admission permits.
    for seq in 1..=17 {
        socket
            .send(frame(
                &json!({"t":"open","seq":seq,"s":Uuid::new_v4(),"from":Uuid::new_v4()}),
                HELLO.as_bytes(),
            ))
            .await
            .unwrap();
    }
    let refusal = routed(&mut socket).await.0;
    assert_eq!(refusal["code"], "capacity");
    assert!(connector.status().await.online);
    connector.stop().await;
}

#[tokio::test]
async fn short_grace_expires_local_sessions_and_requires_a_new_ticket() {
    let center = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let connector = start(&local, &center).await;
    let echo = tokio::spawn(local_echo(local));
    let mut socket = center_socket(&center, &format!("Bearer {}", "a".repeat(64))).await;
    next(&mut socket).await;
    socket
        .send(frame(
            &json!({"t":"welcome","v":1,"link":Uuid::new_v4(),"node_id":Uuid::new_v4(),
        "resumed":false,"recv":0,"resume_token":"b".repeat(64),"grace_seconds":1}),
            &[],
        ))
        .await
        .unwrap();
    socket
        .send(frame(
            &json!({"t":"open","seq":1,"s":Uuid::new_v4(),"from":Uuid::new_v4()}),
            HELLO.as_bytes(),
        ))
        .await
        .unwrap();
    routed(&mut socket).await;
    drop(socket);
    drop(center);
    timeout(Duration::from_secs(5), echo)
        .await
        .unwrap()
        .unwrap();
    timeout(Duration::from_secs(2), async {
        loop {
            if connector.status().await.error == Some("resume_expired") {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
