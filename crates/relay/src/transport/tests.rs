use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;

use super::*;
use crate::protocol::DownloadAck;

#[tokio::test]
async fn typed_receive_answers_pings_and_send_preserves_frames() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        socket.send(Message::Ping(vec![1, 2].into())).await.unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap(),
            Message::Pong(vec![1, 2].into())
        );
        socket.send(Message::Pong(Vec::new().into())).await.unwrap();
        socket
            .send(Message::Text(r#"{"type":"download.complete"}"#.into()))
            .await
            .unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap(),
            Message::Binary(vec![3, 4].into())
        );
        socket.send(Message::Binary(vec![5].into())).await.unwrap();
    });
    let mut socket = connect(&format!("ws://{address}/data"), "ticket", 1024)
        .await
        .unwrap();
    let _: DownloadAck = timeout(Duration::from_secs(3), receive(&mut socket))
        .await
        .unwrap()
        .unwrap();
    send(&mut socket, Message::Binary(vec![3, 4].into()))
        .await
        .unwrap();
    assert!(matches!(text(&mut socket).await, Err(Error::Transport)));
    server.await.unwrap();
}

#[tokio::test]
async fn upgrade_rejects_invalid_credentials_and_bounded_messages() {
    assert!(matches!(
        connect("invalid url", "ticket", 1024).await,
        Err(Error::InvalidGrant)
    ));
    assert!(matches!(
        connect("ws://127.0.0.1:1", "bad\nticket", 1024).await,
        Err(Error::InvalidGrant)
    ));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        socket
            .send(Message::Text("x".repeat(65).into()))
            .await
            .unwrap();
    });
    let mut socket = connect(&format!("ws://{address}"), "ticket", 64)
        .await
        .unwrap();
    assert!(matches!(text(&mut socket).await, Err(Error::Transport)));
    server.await.unwrap();
}
