use super::*;

#[tokio::test]
async fn hub_control_is_negotiated_and_returns_only_public_runtime_state() {
    let fixture = Fixture::start().await;
    let mut socket = fixture.socket().await;
    send(
        &mut socket,
        json!({"type":"hello","client_id":"hub-client",
        "protocol":{"major":1,"min_minor":0,"max_minor":0},
        "capabilities":hub::rpc::METHODS.iter().map(|method| method.name).collect::<Vec<_>>(),
        "required_capabilities":["connection.single.v1"]}),
    )
    .await;
    assert_eq!(receive(&mut socket).await["type"], "server_info");
    let status = request(&mut socket, "hub.status.request", json!({})).await;
    assert_eq!(status["result"]["serverId"], "stable");
    assert_eq!(status["result"]["instanceId"], "instance");
    assert_eq!(status["result"]["status"]["online"], false);
    let invalid = request(
        &mut socket,
        "hub.start.request",
        json!({
            "center_url":"https://example.invalid/api", "hub_ticket":"invalid",
            "node_session_id":"00000000-0000-0000-0000-000000000001"
        }),
    )
    .await;
    assert_eq!(invalid["code"], "invalid_message");
    let unsafe_center = request(
        &mut socket,
        "hub.start.request",
        json!({
            "center_url":"http://example.invalid/api", "hub_ticket":"a".repeat(64),
            "node_session_id":"00000000-0000-0000-0000-000000000001"
        }),
    )
    .await;
    assert_eq!(unsafe_center["code"], "invalid_message");
    let started = request(
        &mut socket,
        "hub.start.request",
        json!({
            "center_url":"http://127.0.0.1:9/api", "hub_ticket":"a".repeat(64),
            "node_session_id":"00000000-0000-0000-0000-000000000001"
        }),
    )
    .await;
    assert_eq!(started["result"]["status"]["connecting"], true);
    let stopped = request(&mut socket, "hub.stop.request", json!({})).await;
    assert_eq!(stopped["result"]["status"]["connecting"], false);
    assert!(!stopped.to_string().contains(TOKEN));
    socket.close(None).await.unwrap();
    fixture.stop().await;
}

#[tokio::test]
async fn hub_control_rejects_requests_without_negotiation() {
    let fixture = Fixture::start().await;
    let mut socket = fixture.socket().await;
    send(&mut socket, hello()).await;
    receive(&mut socket).await;
    let response = request(&mut socket, "hub.stop.request", json!({})).await;
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
        json!({"type":"hello", "client_id":"hub-client",
            "protocol":{"major":1,"min_minor":0,"max_minor":0},
            "capabilities":hub::rpc::METHODS.iter().map(|method| method.name).collect::<Vec<_>>(),
            "required_capabilities":["connection.single.v1"]}),
    )
    .await;
    receive(&mut socket).await;
    // Keep the mock center's handshake pending until a control transport cancels it.
    let center = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let grant = json!({
        "center_url":format!("http://{}/api", center.local_addr().unwrap()),
        "hub_ticket":"a".repeat(64),
        "node_session_id":"00000000-0000-0000-0000-000000000001"
    });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let endpoint = fixture.url("/api/hub/control");

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
    assert!(!fixture.api.shared.hub.status().await.connecting);
    let started = request(&mut socket, "hub.start.request", grant.clone()).await;
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
    let stopped = request(&mut socket, "hub.status.request", json!({})).await;
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
    let restarted = request(&mut socket, "hub.status.request", json!({})).await;
    assert_eq!(restarted["result"]["status"]["connecting"], true);
    let stopped = request(&mut socket, "hub.stop.request", json!({})).await;
    assert_eq!(stopped["result"]["status"]["connecting"], false);
    socket.close(None).await.unwrap();
    fixture.stop().await;
}

#[tokio::test]
async fn managed_owner_rejects_external_http_and_rpc_control_but_allows_status() {
    let fixture = Fixture::with_policy(Services::default(), Vec::new(), true).await;
    let client = reqwest::Client::new();
    for method in [Method::PUT, Method::DELETE] {
        let result = client.request(method, fixture.url("/api/hub/control")).bearer_auth(TOKEN)
            .json(&json!({"center_url":"https://example.invalid/api","hub_ticket":"a".repeat(64),"node_session_id":uuid::Uuid::new_v4()})).send().await.unwrap();
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
    let mut socket = fixture.socket().await;
    send(&mut socket, json!({"type":"hello","client_id":"managed-test","protocol":{"major":1,"min_minor":0,"max_minor":0},
        "capabilities":hub::rpc::METHODS.iter().map(|method| method.name).collect::<Vec<_>>(),"required_capabilities":["connection.single.v1"]})).await;
    receive(&mut socket).await;
    for method in ["hub.start.request", "hub.stop.request"] {
        let response = request(&mut socket, method, json!({})).await;
        assert_eq!(response["code"], "hub_managed");
        assert_eq!(response["message"], model::ErrorCode::HubManaged.message());
    }
    let response = request(&mut socket, "hub.status.request", json!({})).await;
    assert_eq!(response["result"]["management"]["mode"], "managed");
    socket.close(None).await.unwrap();
    fixture.stop().await;
}

#[tokio::test]
async fn internal_claim_is_unique_and_reports_only_public_binding() {
    use host_link::ManagedHub;
    let mut api = Api::new(
        "127.0.0.1:7316".parse().unwrap(),
        "stable".into(),
        "instance".into(),
        TOKEN.into(),
        Services::default(),
    )
    .unwrap();
    let handle = api.claim_managed_hub().unwrap();
    assert!(api.claim_managed_hub().is_err());
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
    assert!(cloned.claim_managed_hub().is_err());
    api.begin_shutdown();
    api.wait_closed().await;
}

mod forwarding;
