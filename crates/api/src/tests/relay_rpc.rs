use super::*;

#[tokio::test]
async fn relay_control_is_negotiated_and_returns_only_public_runtime_state() {
    let fixture = Fixture::start().await;
    let mut socket = fixture.socket().await;
    send(
        &mut socket,
        json!({"type":"hello","client_id":"relay-client",
        "protocol":{"major":1,"min_minor":0,"max_minor":0},
        "capabilities":relay::rpc::METHODS.iter().map(|method| method.name).collect::<Vec<_>>(),
        "required_capabilities":["connection.single.v1"]}),
    )
    .await;
    assert_eq!(receive(&mut socket).await["type"], "server_info");
    let status = request(&mut socket, "relay.status.request", json!({})).await;
    assert_eq!(status["result"]["serverId"], "stable");
    assert_eq!(status["result"]["instanceId"], "instance");
    assert_eq!(status["result"]["status"]["online"], false);
    let invalid = request(
        &mut socket,
        "relay.start.request",
        json!({
            "center_url":"https://example.invalid/api", "control_ticket":"invalid",
            "node_session_id":"00000000-0000-0000-0000-000000000001"
        }),
    )
    .await;
    assert_eq!(invalid["code"], "invalid_message");
    let unsafe_center = request(
        &mut socket,
        "relay.start.request",
        json!({
            "center_url":"http://example.invalid/api", "control_ticket":"a".repeat(64),
            "node_session_id":"00000000-0000-0000-0000-000000000001"
        }),
    )
    .await;
    assert_eq!(unsafe_center["code"], "invalid_message");
    let started = request(
        &mut socket,
        "relay.start.request",
        json!({
            "center_url":"http://127.0.0.1:9/api", "control_ticket":"a".repeat(64),
            "node_session_id":"00000000-0000-0000-0000-000000000001"
        }),
    )
    .await;
    assert_eq!(started["result"]["status"]["connecting"], true);
    let stopped = request(&mut socket, "relay.stop.request", json!({})).await;
    assert_eq!(stopped["result"]["status"]["connecting"], false);
    assert!(!stopped.to_string().contains(TOKEN));
    socket.close(None).await.unwrap();
    fixture.stop().await;
}

#[tokio::test]
async fn relay_control_rejects_requests_without_negotiation() {
    let fixture = Fixture::start().await;
    let mut socket = fixture.socket().await;
    send(&mut socket, hello()).await;
    receive(&mut socket).await;
    let response = request(&mut socket, "relay.stop.request", json!({})).await;
    assert_eq!(response["code"], "unsupported_capability");
    socket.close(None).await.unwrap();
    fixture.stop().await;
}

#[tokio::test]
async fn http_and_rpc_control_the_same_connector_behind_http_authentication() {
    let fixture = Fixture::start().await;
    let mut socket = fixture.socket().await;
    send(
        &mut socket,
        json!({"type":"hello", "client_id":"relay-client",
            "protocol":{"major":1,"min_minor":0,"max_minor":0},
            "capabilities":relay::rpc::METHODS.iter().map(|method| method.name).collect::<Vec<_>>(),
            "required_capabilities":["connection.single.v1"]}),
    )
    .await;
    receive(&mut socket).await;
    // Keep the mock center's handshake pending until a control transport cancels it.
    let center = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let grant = json!({
        "center_url":format!("http://{}/api", center.local_addr().unwrap()),
        "control_ticket":"a".repeat(64),
        "node_session_id":"00000000-0000-0000-0000-000000000001"
    });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let endpoint = fixture.url("/api/relay/control");

    assert_eq!(
        client
            .put(&endpoint)
            .json(&grant)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert!(!fixture.api.shared.relay.status().await.connecting);
    let started = request(&mut socket, "relay.start.request", grant.clone()).await;
    assert_eq!(started["result"]["status"]["connecting"], true);
    let http_status: Value = client
        .get(&endpoint)
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(http_status["connecting"], true);
    assert_eq!(
        client
            .delete(&endpoint)
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    let stopped = request(&mut socket, "relay.status.request", json!({})).await;
    assert_eq!(stopped["result"]["status"]["connecting"], false);

    assert_eq!(
        client
            .put(&endpoint)
            .bearer_auth(TOKEN)
            .json(&grant)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    let restarted = request(&mut socket, "relay.status.request", json!({})).await;
    assert_eq!(restarted["result"]["status"]["connecting"], true);
    let stopped = request(&mut socket, "relay.stop.request", json!({})).await;
    assert_eq!(stopped["result"]["status"]["connecting"], false);
    socket.close(None).await.unwrap();
    fixture.stop().await;
}
