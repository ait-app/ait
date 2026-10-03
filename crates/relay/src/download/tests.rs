use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;

use super::*;

async fn transfer(status: u16, ack: &str) -> Result<(), Error> {
    let center = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let session = Uuid::new_v4();
    let destination = Arc::new(Local {
        url: format!("ws://{}/v1/ws", local.local_addr().unwrap()),
        token: SecretString::from("local-bearer"),
        server_id: "server".to_owned(),
        instance_id: "instance".to_owned(),
    });
    let body = vec![42_u8; 128 * 1024 + 17];
    let expected = body.clone();
    let local_task = tokio::spawn(async move {
        let (mut tcp, _) = local.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(tcp.read_u8().await.unwrap());
            assert!(request.len() < 4096);
        }
        let request = String::from_utf8(request).unwrap();
        assert!(request.starts_with("GET /api/files/download?token=scoped-token HTTP/1.1\r\n"));
        assert!(!request.contains("relay-ticket"));
        let body = if status == 200 { body } else { Vec::new() };
        let header = format!(
            "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
            body.len()
        );
        tcp.write_all(header.as_bytes()).await.unwrap();
        tcp.write_all(&body).await.unwrap();
    });
    let url = format!("ws://{}/data", center.local_addr().unwrap());
    let task = tokio::spawn(run(
        url,
        SecretString::from("relay-ticket"),
        SecretString::from("scoped-token"),
        destination,
        session,
    ));
    let (tcp, _) = center.accept().await.unwrap();
    let mut socket = accept_async(tcp).await.unwrap();
    socket
        .send(Message::Text(
            json!({"type":"relay.ready","relay_session_id":session})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    if status == 200 {
        let header = socket.next().await.unwrap().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(header.to_text().unwrap()).unwrap(),
            json!({"type":"download.headers","status":200,"content_length":expected.len(),
                "content_type":"application/octet-stream"})
        );
        let mut received = Vec::new();
        loop {
            match socket.next().await.unwrap().unwrap() {
                Message::Binary(bytes) => {
                    assert!(bytes.len() <= 64 * 1024);
                    received.extend_from_slice(&bytes);
                }
                Message::Text(text) => {
                    assert_eq!(
                        serde_json::from_str::<Value>(&text).unwrap(),
                        json!({"type":"download.end","bytes":expected.len()})
                    );
                    break;
                }
                message => panic!("unexpected download frame: {message:?}"),
            }
        }
        assert_eq!(received, expected);
        assert!(
            !task.is_finished(),
            "download must wait for destination acknowledgement"
        );
        socket
            .send(Message::Text(ack.to_owned().into()))
            .await
            .unwrap();
    }
    let result = task.await.unwrap();
    local_task.await.unwrap();
    result
}

#[tokio::test]
async fn download_stream_preserves_headers_chunks_and_final_acknowledgement() {
    timeout(
        Duration::from_secs(5),
        transfer(200, r#"{"type":"download.complete"}"#),
    )
    .await
    .unwrap()
    .unwrap();
}

#[tokio::test]
async fn download_rejects_wrong_acknowledgement_and_http_failure() {
    let result = timeout(
        Duration::from_secs(5),
        transfer(200, r#"{"type":"download.end"}"#),
    )
    .await
    .unwrap();
    assert!(matches!(result, Err(Error::Transport)));
    let result = timeout(Duration::from_secs(5), transfer(403, ""))
        .await
        .unwrap();
    assert!(matches!(result, Err(Error::Protocol)));
}
