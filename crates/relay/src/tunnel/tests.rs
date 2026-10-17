use super::*;
use crate::{Connector, ControlGrant};
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio_tungstenite::{WebSocketStream, accept_async};

async fn next_json(socket: &mut WebSocketStream<TcpStream>) -> Value {
    loop {
        match timeout(Duration::from_secs(5), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
        {
            Message::Text(body) => return serde_json::from_str(&body).unwrap(),
            Message::Ping(body) => socket.send(Message::Pong(body)).await.unwrap(),
            _ => {}
        }
    }
}

async fn center_connection(
    socket: WebSocketStream<TcpStream>,
    receiver: mpsc::Receiver<mux::OpenRequest>,
) {
    let (sink, stream) = socket.split();
    let sink = sink
        .sink_map_err(|_| wire::invalid())
        .with(|frame| future::ready(Ok(to_message(frame))));
    let stream = stream.map(|message| match message.map_err(|_| wire::invalid())? {
        Message::Binary(body) => Ok(mux::Frame::Binary(body.to_vec())),
        Message::Ping(body) => Ok(mux::Frame::Ping(body.to_vec())),
        Message::Pong(body) => Ok(mux::Frame::Pong(body.to_vec())),
        Message::Close(_) => Ok(mux::Frame::Close),
        _ => Err(wire::invalid()),
    });
    let (network, yamux) = tokio::io::duplex(wire::MAX_BLOCK);
    tokio::select! {
        _ = mux::transport(sink, stream, network) => {},
        _ = mux::drive(yamux, yamux::Mode::Client, receiver, None) => {},
    }
}

async fn echo_local(listener: TcpListener) {
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            _ = tasks.join_next(), if !tasks.is_empty() => {},
            incoming = listener.accept() => {
                let (stream, _) = incoming.unwrap();
                tasks.spawn(async move {
                    let mut ws = accept_async(stream).await.unwrap();
                    while let Some(Ok(message)) = ws.next().await {
                        match message {
                            Message::Text(_) | Message::Binary(_) => {
                                if ws.send(message).await.is_err() { break; }
                            },
                            Message::Ping(body) => { let _ = ws.send(Message::Pong(body)).await; },
                            Message::Close(_) => break,
                            _ => {},
                        }
                    }
                });
            },
        }
    }
}

#[tokio::test]
async fn one_wss_serves_one_hundred_visits_and_preserves_large_binary_messages() {
    let center = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let connector = Connector::new(
        local.local_addr().unwrap(),
        SecretString::from("local-only"),
        Uuid::new_v4().to_string(),
        Uuid::new_v4().to_string(),
    );
    let local_task = tokio::spawn(echo_local(local));
    connector
        .start(ControlGrant {
            center_url: format!("http://{}", center.local_addr().unwrap()),
            control_ticket: "a".repeat(64),
            node_session_id: Uuid::new_v4(),
        })
        .await
        .unwrap();
    let (tcp, _) = center.accept().await.unwrap();
    let mut control = accept_async(tcp).await.unwrap();
    let hello = next_json(&mut control).await;
    assert_eq!(hello["version"], 1);
    assert!(hello.get("supported_tunnel_protocols").is_none());
    let epoch = Uuid::new_v4();
    control
        .send(Message::Text(
            json!({"type":"control.welcome","epoch":epoch,"supported_tunnel_protocols":[PROTOCOL]})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    let request = next_json(&mut control).await;
    assert_eq!(request["type"], "tunnel.request");
    let tunnel_id = Uuid::new_v4();
    control.send(Message::Text(json!({"type":"tunnel.grant","request_id":request["request_id"],"epoch":epoch,"protocol":PROTOCOL,"tunnel_id":tunnel_id,"tunnel_ticket":"b".repeat(64)}).to_string().into())).await.unwrap();
    let (tcp, _) = center.accept().await.unwrap();
    let mut socket = accept_async(tcp).await.unwrap();
    let prelude = next_json(&mut socket).await;
    assert_eq!(prelude["tunnel_id"], tunnel_id.to_string());
    socket
        .send(Message::Text(prelude.to_string().into()))
        .await
        .unwrap();
    let (requests, receiver) = mpsc::channel(16);
    let driver = tokio::spawn(center_connection(socket, receiver));
    timeout(Duration::from_secs(20), async {
        for index in 0..100 {
            let (send, receive) = oneshot::channel();
            requests.send(send).await.unwrap();
            let mut stream = receive.await.unwrap();
            let open = wire::Open {
                relay_session_id: Uuid::new_v4(),
                source_node_session_id: Uuid::new_v4(),
                epoch,
                tunnel_id,
                mode: "ait-rust-single-v1".into(),
            };
            wire::write(&mut stream, wire::OPEN, &serde_json::to_vec(&open).unwrap())
                .await
                .unwrap();
            wire::expect(&mut stream, wire::ACCEPT).await.unwrap();
            wire::write(&mut stream, wire::START, &[]).await.unwrap();
            let hello = br#"{ "type": "hello", "future": true }"#;
            wire::write(&mut stream, wire::TEXT, hello).await.unwrap();
            assert_eq!(
                wire::read(&mut stream).await.unwrap(),
                (wire::TEXT, hello.to_vec())
            );
            if index == 0 {
                let body = vec![197; wire::MAX_BODY];
                wire::write(&mut stream, wire::BINARY, &body).await.unwrap();
                assert_eq!(wire::read(&mut stream).await.unwrap(), (wire::BINARY, body));
            }
            wire::write(&mut stream, wire::CLOSE, &[]).await.unwrap();
        }
    })
    .await
    .unwrap();
    assert!(
        timeout(Duration::from_millis(50), center.accept())
            .await
            .is_err()
    );
    assert!(connector.status().await.online);
    connector.stop().await;
    timeout(Duration::from_secs(3), driver)
        .await
        .unwrap()
        .unwrap();
    local_task.abort();
}

#[tokio::test]
async fn rejected_tunnel_does_not_close_control_or_change_online_state() {
    let center = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let connector = Connector::new(
        "127.0.0.1:1".parse().unwrap(),
        SecretString::from("local"),
        Uuid::new_v4().to_string(),
        Uuid::new_v4().to_string(),
    );
    connector
        .start(ControlGrant {
            center_url: format!("http://{}", center.local_addr().unwrap()),
            control_ticket: "a".repeat(64),
            node_session_id: Uuid::new_v4(),
        })
        .await
        .unwrap();
    let (tcp, _) = center.accept().await.unwrap();
    let mut control = accept_async(tcp).await.unwrap();
    next_json(&mut control).await;
    let epoch = Uuid::new_v4();
    control
        .send(Message::Text(
            json!({"type":"control.welcome","epoch":epoch,"supported_tunnel_protocols":[PROTOCOL]})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    let request = next_json(&mut control).await;
    control.send(Message::Text(json!({"type":"tunnel.error","request_id":request["request_id"],"code":"relay_capacity"}).to_string().into())).await.unwrap();
    control
        .send(Message::Ping(vec![4, 2].into()))
        .await
        .unwrap();
    let pong = timeout(Duration::from_secs(3), control.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(pong, Message::Pong(vec![4, 2].into()));
    assert!(connector.status().await.online);
    assert!(
        timeout(Duration::from_millis(50), center.accept())
            .await
            .is_err()
    );
    connector.stop().await;
}
