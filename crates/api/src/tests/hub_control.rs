use super::*;

fn grant(center: &str) -> Value {
    json!({
        "center_url":center,
        "hub_ticket":"a".repeat(64),
        "node_session_id":uuid::Uuid::new_v4()
    })
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
    assert!(
        !fixture.api.shared.relay.status().await.connecting,
        "the legacy relay is independent"
    );
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
