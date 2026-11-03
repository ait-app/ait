use super::*;

#[tokio::test]
async fn mcp_socket_router_limits_routes_and_enforces_source_and_bearer_policy() {
    let fixture = Fixture::start().await;
    let root = tempfile::tempdir_in("/tmp").unwrap();
    let path = root.path().join("mcp.sock");
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    let router = fixture.api.mcp_socket_router();
    let shutdown = fixture.api.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move { shutdown.wait_draining().await })
            .await
            .unwrap();
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .unix_socket(path.as_path())
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let request = || {
        client
            .post("http://localhost/mcp/agents")
            .header("accept", "application/json, text/event-stream")
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"ping"}))
    };
    for token in [TOKEN, AGENT_MCP_TOKEN] {
        let response = request().bearer_auth(token).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.json::<Value>().await.unwrap()["result"], json!({}));
    }
    assert_eq!(
        request().send().await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request()
            .bearer_auth("wrong")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    for (header, value, status) in [
        ("host", "evil.test", StatusCode::FORBIDDEN),
        ("origin", "http://evil.test", StatusCode::FORBIDDEN),
        ("accept", "application/json", StatusCode::NOT_ACCEPTABLE),
        ("mcp-protocol-version", "future", StatusCode::BAD_REQUEST),
    ] {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::HeaderName::from_bytes(header.as_bytes()).unwrap(),
            value.parse().unwrap(),
        );
        assert_eq!(
            request()
                .bearer_auth(TOKEN)
                .headers(headers)
                .send()
                .await
                .unwrap()
                .status(),
            status
        );
    }
    assert_eq!(
        request()
            .bearer_auth(TOKEN)
            .query(&[("token", "wrong")])
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    for path in ["/healthz", "/v1/ws", "/v1/server/info"] {
        assert_eq!(
            client
                .get(format!("http://localhost{path}"))
                .bearer_auth(TOKEN)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        client
            .get("http://localhost/mcp/agents")
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
    fixture.api.begin_shutdown();
    task.await.unwrap();
    fixture.stop().await;
}
