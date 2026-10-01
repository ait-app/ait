use super::*;

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
async fn control_opens_independent_reverse_data_and_keeps_local_token_private() {
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    let center = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let connector = Connector::new(
        local.local_addr().unwrap(),
        SecretString::from("private-local-bearer"),
        Uuid::new_v4().to_string(),
        Uuid::new_v4().to_string(),
    );
    let local_task = tokio::spawn(mock_local(local));
    connector
        .start(ControlGrant {
            center_url: format!("http://{}", center.local_addr().unwrap()),
            control_ticket: "a".repeat(64),
            node_session_id: Uuid::new_v4(),
        })
        .await
        .unwrap();
    let (tcp, _) = timeout(Duration::from_secs(3), center.accept())
        .await
        .unwrap()
        .unwrap();
    let mut control = accept_async(tcp).await.unwrap();
    let hello = control.next().await.unwrap().unwrap();
    assert!(!hello.to_text().unwrap().contains("private-local-bearer"));
    let epoch = Uuid::new_v4();
    control
        .send(Message::Text(
            json!({"type":"control.welcome","epoch":epoch})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    // There is no data connection before open_data.
    assert!(
        timeout(Duration::from_millis(50), center.accept())
            .await
            .is_err()
    );
    let id = Uuid::new_v4();
    control
        .send(Message::Text(
            json!({"type":"open_data","relay_session_id":id,"daemon_ticket":"b".repeat(64),
        "epoch":epoch,"mode":"ait-rust-single-v1"})
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    let (tcp, _) = timeout(Duration::from_secs(3), center.accept())
        .await
        .unwrap()
        .unwrap();
    let mut data = accept_async(tcp).await.unwrap();
    data.send(Message::Text(
        json!({"type":"relay.ready","relay_session_id":id})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();
    data.send(Message::Text(
        json!({"type":"hello","client_id":"test"})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();
    assert_eq!(
        timeout(Duration::from_secs(3), data.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        Message::Text("native server info".into())
    );
    assert_eq!(
        data.next().await.unwrap().unwrap(),
        Message::Binary(vec![0, 255, 1].into())
    );
    data.send(Message::Text("client request".into()))
        .await
        .unwrap();
    timeout(Duration::from_secs(3), local_task)
        .await
        .unwrap()
        .unwrap();
    connector.stop().await;
    assert!(!connector.status().await.online);
    assert!(matches!(
        timeout(Duration::from_secs(3), control.next())
            .await
            .unwrap(),
        None | Some(Err(_) | Ok(Message::Close(_)))
    ));
}

async fn mock_local(local: tokio::net::TcpListener) {
    use tokio_tungstenite::accept_hdr_async;
    use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
    let (tcp, _) = local.accept().await.unwrap();
    let mut socket = accept_hdr_async(tcp, |request: &Request, response: Response| {
        assert_eq!(request.uri().path(), "/v1/ws");
        assert_eq!(
            request.headers()["authorization"],
            "Bearer private-local-bearer"
        );
        Ok(response)
    })
    .await
    .unwrap();
    let hello = socket.next().await.unwrap().unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(hello.to_text().unwrap()).unwrap()["type"],
        "hello"
    );
    socket
        .send(Message::Text("native server info".into()))
        .await
        .unwrap();
    socket
        .send(Message::Binary(vec![0, 255, 1].into()))
        .await
        .unwrap();
    let reply = socket.next().await.unwrap().unwrap();
    assert_eq!(reply, Message::Text("client request".into()));
}
