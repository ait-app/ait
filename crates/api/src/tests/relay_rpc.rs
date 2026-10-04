use super::*;

#[tokio::test]
async fn relay_control_is_negotiated_and_returns_only_public_runtime_state() {
    let fixture = Fixture::start().await;
    let mut socket = fixture.socket().await;
    send(
        &mut socket,
        json!({"type":"hello","client_id":"relay-client",
        "protocol":{"major":1,"min_minor":0,"max_minor":0},
        "capabilities":crate::relay_rpc::METHODS,
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
