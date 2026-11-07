use super::*;

fn grant(center: &str) -> Value {
    json!({
        "center_url":center,
        "hub_ticket":"a".repeat(64),
        "node_session_id":uuid::Uuid::new_v4(),
        "hub_features":["lease", "hosts", "state", "credit", "download", "bulk"]
    })
}

#[tokio::test]
async fn hub_grant_without_the_required_features_is_refused() {
    let fixture = Fixture::start().await;
    let client = reqwest::Client::new();
    let endpoint = fixture.url("/api/hub/control");
    let mut missing = grant("http://127.0.0.1:9");
    missing.as_object_mut().unwrap().remove("hub_features");
    let mut partial = grant("http://127.0.0.1:9");
    partial["hub_features"] = json!(["lease", "credit"]);
    for body in [missing, partial] {
        let response = client
            .put(&endpoint)
            .bearer_auth(TOKEN)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    assert!(!fixture.api.shared.relay.hub().status().await.connecting);
    fixture.stop().await;
}

#[tokio::test]
async fn hub_link_is_controlled_over_authenticated_local_http() {
    let fixture = Fixture::start().await;
    // Keep the mock center's handshake pending until the link is stopped.
    let center = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let center = format!("http://{}", center.local_addr().unwrap());
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let endpoint = fixture.url("/api/hub/control");

    let unauthenticated = client
        .put(&endpoint)
        .json(&grant(&center))
        .send()
        .await
        .unwrap();
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
    let invalid = client
        .put(&endpoint)
        .bearer_auth(TOKEN)
        .json(&grant("http://example.com"))
        .send()
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    let started = client
        .put(&endpoint)
        .bearer_auth(TOKEN)
        .json(&grant(&center))
        .send()
        .await
        .unwrap();
    assert_eq!(started.status(), StatusCode::ACCEPTED);

    let status: Value = client
        .get(&endpoint)
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["connecting"], true);
    assert_eq!(status["online"], false);
    assert_eq!(status["management"]["mode"], "external");
    assert!(!status.to_string().contains(&"a".repeat(64)));
    let stopped = client
        .delete(&endpoint)
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(stopped.status(), StatusCode::NO_CONTENT);
    assert!(!fixture.api.shared.relay.hub().status().await.connecting);
    fixture.stop().await;
}

#[tokio::test]
async fn managed_owner_rejects_external_hub_control() {
    let fixture = Fixture::with_policy(Services::default(), Vec::new(), true).await;
    let client = reqwest::Client::new();
    for method in [Method::PUT, Method::DELETE] {
        let result = client
            .request(method, fixture.url("/api/hub/control"))
            .bearer_auth(TOKEN)
            .json(&grant("https://example.invalid"))
            .send()
            .await
            .unwrap();
        assert_eq!(result.status(), StatusCode::CONFLICT);
    }
    let status: Value = client
        .get(fixture.url("/api/hub/control"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["management"]["mode"], "managed");
    fixture.stop().await;
}

fn hub_frame(header: &Value, payload: &[u8]) -> Message {
    let header = serde_json::to_vec(header).unwrap();
    let mut bytes = u16::try_from(header.len()).unwrap().to_be_bytes().to_vec();
    bytes.extend_from_slice(&header);
    bytes.extend_from_slice(payload);
    Message::Binary(bytes.into())
}

/// Next non-ack Hub frame from the Host as (header, payload).
async fn hub_next(center: &mut WebSocketStream<TcpStream>) -> (Value, Vec<u8>) {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(3), center.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let Message::Binary(bytes) = message else {
            continue;
        };
        let length = usize::from(u16::from_be_bytes([bytes[0], bytes[1]]));
        let header: Value = serde_json::from_slice(&bytes[2..2 + length]).unwrap();
        if header["t"] != "ack" {
            return (header, bytes[2 + length..].to_vec());
        }
    }
}

#[tokio::test]
async fn hub_sessions_are_served_in_process_by_this_api() {
    let fixture = Fixture::start().await;
    let center = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut grant = grant(&format!("http://{}", center.local_addr().unwrap()));
    grant["hub_features"] = json!(["lease", "credit", "download"]);
    let started = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .put(fixture.url("/api/hub/control"))
        .bearer_auth(TOKEN)
        .json(&grant)
        .send()
        .await
        .unwrap();
    assert_eq!(started.status(), StatusCode::ACCEPTED);
    let (tcp, _) = center.accept().await.unwrap();
    let mut link = tokio_tungstenite::accept_async(tcp).await.unwrap();
    assert_eq!(
        hub_next(&mut link).await.0["f"],
        json!(["lease", "credit", "download"])
    );
    link.send(hub_frame(
        &json!({"t":"welcome","link":uuid::Uuid::new_v4(),"resumed":false,"recv":0,
            "resume_token":"b".repeat(64),"grace_seconds":120,"f":["lease","credit","download"],"win":262_144}),
        b"",
    ))
    .await
    .unwrap();

    let s = uuid::Uuid::new_v4();
    let mut hello = hello();
    hello["required_capabilities"] = json!(["connection.single.v1"]);
    link.send(hub_frame(
        &json!({"t":"open","seq":1,"s":s,"from":uuid::Uuid::new_v4(),"w":262_144}),
        hello.to_string().as_bytes(),
    ))
    .await
    .unwrap();
    let (accept, info) = hub_next(&mut link).await;
    assert_eq!(accept, json!({"t":"accept","seq":1,"s":s,"w":1_048_576}));
    let info: Value = serde_json::from_slice(&info).unwrap();
    assert_eq!(info["type"], "server_info");
    assert_eq!(info["info"]["server_id"], "stable");

    link.send(hub_frame(
        &json!({"t":"msg","seq":2,"s":s,"pt":"text"}),
        json!({"type":"request","request_id":"r1","method":"connection.ping",
            "params":{"nonce":"hub"}})
        .to_string()
        .as_bytes(),
    ))
    .await
    .unwrap();
    // The delivered request is credited at once, before or after the reply.
    let (header, reply) = loop {
        let (header, payload) = hub_next(&mut link).await;
        if header["t"] != "credit" {
            break (header, payload);
        }
    };
    assert_eq!(header, json!({"t":"msg","seq":2,"s":s,"pt":"text"}));
    let reply: Value = serde_json::from_slice(&reply).unwrap();
    assert_eq!(reply["result"]["nonce"], "hub");
    fixture.stop().await;
}
