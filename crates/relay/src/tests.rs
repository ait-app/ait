use super::*;

use futures_util::SinkExt;
use serde_json::json;
mod lifecycle;

#[test]
fn center_urls_require_tls_and_preserve_proxy_prefix() {
    assert!(center_url("http://example.com").is_err());
    assert!(center_url("https://user:password@example.com").is_err());
    assert!(center_url("https://example.com?token=secret").is_err());
    assert!(center_url("file:///tmp/center").is_err());
    assert!(center_url("http://127.0.0.1:3000").is_ok());
    let center = center_url("https://example.com/api").unwrap();
    assert_eq!(
        endpoint(&center, "v1/relay/control"),
        "wss://example.com/api/v1/relay/control"
    );
}

#[tokio::test]
async fn stop_is_idempotent_and_bad_tickets_never_start() {
    let connector = Connector::new(
        "127.0.0.1:12345".parse().unwrap(),
        SecretString::from("local-only"),
        Uuid::new_v4().to_string(),
        Uuid::new_v4().to_string(),
    );
    assert!(!connector.status().await.online);
    let result = connector
        .start(ControlGrant {
            center_url: "https://example.com".to_owned(),
            control_ticket: "short".to_owned(),
            node_session_id: Uuid::new_v4(),
        })
        .await;
    assert!(result.is_err());
    connector.stop().await;
    connector.stop().await;
    assert!(!connector.status().await.connecting);
}

#[tokio::test]
async fn control_ignores_tunnel_advertisements_and_still_streams_downloads() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    let exercise = async {
        let center = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let connector = Connector::new(
            local.local_addr().unwrap(),
            "private-local-bearer".into(),
            "server".into(),
            "instance".into(),
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
        let hello = control.next().await.unwrap().unwrap();
        assert!(!hello.to_text().unwrap().contains("private-local-bearer"));
        let epoch = Uuid::new_v4();
        control
            .send(Message::Text(
                json!({
                    "type":"control.welcome", "epoch":epoch,
                    "supported_tunnel_protocols":["ait-relay-tunnel-v1"]
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        control.send(Message::Ping(vec![1].into())).await.unwrap();
        // No tunnel.request is sent, even when the center advertises the retired path.
        assert_eq!(
            control.next().await.unwrap().unwrap(),
            Message::Pong(vec![1].into())
        );
        let session = Uuid::new_v4();
        let download = json!({
            "type":"open_data", "relay_session_id":session, "epoch":epoch,
            "daemon_ticket":"b".repeat(64), "mode":"ait-download-v1",
            "download_token":"scoped-token"
        });
        control
            .send(Message::Text(download.to_string().into()))
            .await
            .unwrap();
        let (tcp, _) = center.accept().await.unwrap();
        let mut data = accept_async(tcp).await.unwrap();
        data.send(Message::Text(
            json!({"type":"relay.ready","relay_session_id":session})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
        let (mut http, _) = local.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(http.read_u8().await.unwrap());
            assert!(request.len() < 4096);
        }
        let request = String::from_utf8(request).unwrap();
        assert!(request.starts_with("GET /api/files/download?token=scoped-token HTTP/1.1\r\n"));
        assert!(!request.contains("private-local-bearer"));
        http.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc")
            .await
            .unwrap();
        let header = data.next().await.unwrap().unwrap();
        assert!(header.to_text().unwrap().contains("download.headers"));
        assert_eq!(
            data.next().await.unwrap().unwrap(),
            Message::Binary(b"abc".to_vec().into())
        );
        let end = data.next().await.unwrap().unwrap();
        assert!(end.to_text().unwrap().contains("download.end"));
        data.send(Message::Text(r#"{"type":"download.complete"}"#.into()))
            .await
            .unwrap();
        connector.stop().await;
        assert!(!connector.status().await.online);
    };
    timeout(Duration::from_secs(5), exercise).await.unwrap();
}
