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

#[tokio::test]
async fn managed_owner_rejects_external_http_and_rpc_control_but_allows_status() {
    let fixture = Fixture::with_policy(Services::default(), Vec::new(), true).await;
    let client = reqwest::Client::new();
    for method in [Method::PUT, Method::DELETE] {
        let result = client.request(method, fixture.url("/api/relay/control")).bearer_auth(TOKEN)
            .json(&json!({"center_url":"https://example.invalid/api","control_ticket":"a".repeat(64),"node_session_id":uuid::Uuid::new_v4()})).send().await.unwrap();
        assert_eq!(result.status(), StatusCode::CONFLICT);
    }
    let status: Value = client
        .get(fixture.url("/api/relay/control"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["management"]["mode"], "managed");
    let mut socket = fixture.socket().await;
    send(&mut socket, json!({"type":"hello","client_id":"managed-test","protocol":{"major":1,"min_minor":0,"max_minor":0},
        "capabilities":crate::relay_rpc::METHODS,"required_capabilities":["connection.single.v1"]})).await;
    receive(&mut socket).await;
    for method in ["relay.start.request", "relay.stop.request"] {
        let response = request(&mut socket, method, json!({})).await;
        assert_eq!(response["code"], "relay_managed");
        assert_eq!(
            response["message"],
            model::ErrorCode::RelayManaged.message()
        );
    }
    let response = request(&mut socket, "relay.status.request", json!({})).await;
    assert_eq!(response["result"]["management"]["mode"], "managed");
    socket.close(None).await.unwrap();
    fixture.stop().await;
}

#[tokio::test]
async fn internal_claim_is_unique_and_reports_only_public_binding() {
    use host_link::ManagedRelay;
    let mut api = Api::new(
        "127.0.0.1:7316".parse().unwrap(),
        "stable".into(),
        "instance".into(),
        TOKEN.into(),
        Services::default(),
    )
    .unwrap();
    let handle = api.claim_managed_relay().unwrap();
    assert!(api.claim_managed_relay().is_err());
    let binding = host_link::Binding {
        node_id: uuid::Uuid::new_v4(),
        host_id: uuid::Uuid::new_v4(),
        server_id: uuid::Uuid::new_v4(),
        grant_id: uuid::Uuid::new_v4(),
    };
    handle.report("reauthorization_required", Some(&binding));
    assert_eq!(
        api.shared.managed_status.lock().unwrap()["phase"],
        "reauthorization_required"
    );
    assert!(!handle.active().await);
    assert_eq!(
        handle
            .start(uuid::Uuid::new_v4(), "invalid-ticket".into())
            .await,
        Err(host_link::Error::Unavailable)
    );
    handle.stop().await;
    let mut cloned = api.clone();
    assert!(cloned.claim_managed_relay().is_err());
    api.begin_shutdown();
    api.wait_closed().await;
}
