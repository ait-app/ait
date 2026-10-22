use super::super::*;
use tokio_tungstenite::accept_async;

fn frame(header: &Value, payload: &[u8]) -> Message {
    let header = header.to_string();
    let mut bytes = u16::try_from(header.len()).unwrap().to_be_bytes().to_vec();
    bytes.extend_from_slice(header.as_bytes());
    bytes.extend_from_slice(payload);
    Message::Binary(bytes.into())
}

async fn routed(socket: &mut WebSocketStream<TcpStream>) -> (Value, Vec<u8>) {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let Message::Binary(bytes) = message else {
            panic!("expected Hub binary envelope")
        };
        let length = usize::from(u16::from_be_bytes([bytes[0], bytes[1]]));
        let header: Value = serde_json::from_slice(&bytes[2..2 + length]).unwrap();
        if header["t"] != "ack" {
            return (header, bytes[2 + length..].to_vec());
        }
    }
}

#[tokio::test]
async fn hub_serve_forwards_real_daemon_hello_and_ping_and_removes_old_control_route() {
    let fixture = Fixture::start().await;
    let center = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    assert_eq!(
        client
            .get(fixture.url("/api/relay/control"))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let response = client
        .put(fixture.url("/api/hub/control"))
        .bearer_auth(TOKEN)
        .json(
            &json!({"center_url":format!("http://{}/api",center.local_addr().unwrap()),
            "hub_ticket":"a".repeat(64),"node_session_id":uuid::Uuid::new_v4()}),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let (tcp, _) = tokio::time::timeout(Duration::from_secs(5), center.accept())
        .await
        .unwrap()
        .unwrap();
    let mut remote = accept_async(tcp).await.unwrap();
    assert_eq!(routed(&mut remote).await.0["t"], "hello");
    remote
        .send(frame(
            &json!({"t":"welcome","v":1,"link":uuid::Uuid::new_v4(),
        "node_id":uuid::Uuid::new_v4(),"resumed":false,"recv":0,
        "resume_token":"b".repeat(64),"grace_seconds":120}),
            &[],
        ))
        .await
        .unwrap();
    let session = uuid::Uuid::new_v4();
    let hello = json!({"type":"hello","client_id":"hub-api-test",
        "protocol":{"major":1,"min_minor":0,"max_minor":0},
        "capabilities":["connection.ping"],"required_capabilities":["connection.single.v1"]});
    remote
        .send(frame(
            &json!({"t":"open","seq":1,"s":session,"from":uuid::Uuid::new_v4()}),
            hello.to_string().as_bytes(),
        ))
        .await
        .unwrap();
    let (accept, info) = routed(&mut remote).await;
    assert_eq!(accept["t"], "accept");
    assert_eq!(
        serde_json::from_slice::<Value>(&info).unwrap()["info"]["server_id"],
        "stable"
    );
    let request = json!({"type":"request","request_id":"ping","method":"connection.ping","params":{"nonce":"hub"}});
    remote
        .send(frame(
            &json!({"t":"msg","seq":2,"s":session,"pt":"text"}),
            request.to_string().as_bytes(),
        ))
        .await
        .unwrap();
    let (header, body) = routed(&mut remote).await;
    assert_eq!(header["s"], session.to_string());
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["result"]["nonce"],
        "hub"
    );
    fixture.stop().await;
}
