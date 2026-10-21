use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use secrecy::SecretString;
use tokio::net::TcpListener;
use tokio::sync::{Semaphore, mpsc};
use tokio::time::timeout;
use tokio_tungstenite::accept_async;

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
        inbound_rx,
        output_tx.clone(),
    )
    .await;
    let (_inbound_tx, inbound_rx) = mpsc::channel(1);
    run(
        local(address),
        Uuid::new_v4(),
        Bytes::from_static(HELLO.as_bytes()),
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
