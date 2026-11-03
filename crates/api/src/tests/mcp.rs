use super::*;

#[cfg(unix)]
mod socket;

#[tokio::test]
async fn agent_mcp_credential_is_private_and_limited_to_its_route() {
    let fixture = Fixture::start().await;
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let request = json!({"jsonrpc":"2.0","id":1,"method":"tools/list"});
    let response = client
        .post(fixture.url(crate::mcp::PATH))
        .bearer_auth(AGENT_MCP_TOKEN)
        .header("accept", "application/json, text/event-stream")
        .json(&request)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value = response.json().await.unwrap();
    assert_eq!(value["result"]["tools"], json!([]));
    for path in ["/v1/server/info", "/v1/ws"] {
        assert_eq!(
            client
                .get(fixture.url(path))
                .bearer_auth(AGENT_MCP_TOKEN)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    let info = client
        .get(fixture.url("/v1/server/info"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(info.status(), StatusCode::OK);
    assert!(!info.text().await.unwrap().contains(AGENT_MCP_TOKEN));
    assert!(!format!("{:?}", fixture.api).contains(AGENT_MCP_TOKEN));
    let rejected = fixture
        .api
        .clone()
        .with_agent_mcp_token(AGENT_MCP_TOKEN.into());
    assert!(matches!(rejected, Err(ConfigError::SharedConfiguration)));
    fixture.stop().await;
}

#[tokio::test]
async fn invalid_agent_mcp_credential_is_rejected_before_serving() {
    let api = Api::new(
        "127.0.0.1:7316".parse().unwrap(),
        "stable".into(),
        "instance".into(),
        TOKEN.into(),
        Services::default(),
    )
    .unwrap();
    assert!(matches!(
        api.with_agent_mcp_token("invalid credential".into()),
        Err(ConfigError::InvalidToken)
    ));
}

fn post(client: &reqwest::Client, fixture: &Fixture, value: &Value) -> reqwest::RequestBuilder {
    client
        .post(fixture.url(crate::mcp::PATH))
        .bearer_auth(TOKEN)
        .header("accept", "application/json, text/event-stream")
        .json(value)
}

#[tokio::test]
async fn mcp_stateless_lifecycle_errors_and_authentication() {
    let fixture = Fixture::start().await;
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let initialize = json!({"jsonrpc":"2.0","id":"init","method":"initialize",
        "params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}});
    let response = post(&client, &fixture, &initialize).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get("mcp-session-id").is_none());
    let result: Value = response.json().await.unwrap();
    assert_eq!(result["id"], "init");
    assert_eq!(result["result"]["protocolVersion"], "2025-11-25");
    let notification = post(
        &client,
        &fixture,
        &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )
    .send()
    .await
    .unwrap();
    assert_eq!(notification.status(), StatusCode::ACCEPTED);
    assert!(notification.bytes().await.unwrap().is_empty());
    let listed: Value = post(
        &client,
        &fixture,
        &json!({"jsonrpc":"2.0","id":7,"method":"tools/list"}),
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(listed["id"], 7);
    assert_eq!(listed["result"]["tools"], json!([]));
    for (method, params, code) in [
        ("unknown", json!({}), -32601),
        ("tools/call", json!({"name":"create_agent"}), -32602),
        ("tools/list", json!({"cursor":"invalid"}), -32602),
        ("initialize", json!({}), -32602),
    ] {
        let result: Value = post(
            &client,
            &fixture,
            &json!({"jsonrpc":"2.0","id":8,"method":method,"params":params}),
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
        assert_eq!(result["error"]["code"], code, "{result}");
    }
    for invalid in [
        &json!([]),
        &json!({"jsonrpc":"1.0","method":"ping","id":1}),
        &json!({"jsonrpc":"2.0","method":"ping","id":null}),
        &json!({"jsonrpc":"2.0","method":"ping","params":false,"id":1}),
    ] {
        let response = post(&client, &fixture, invalid).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response.json::<Value>().await.unwrap()["error"]["code"],
            -32600
        );
    }
    let malformed = client
        .post(fixture.url(crate::mcp::PATH))
        .bearer_auth(TOKEN)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body("{")
        .send()
        .await
        .unwrap();
    assert_eq!(
        malformed.json::<Value>().await.unwrap()["error"]["code"],
        -32700
    );
    fixture.stop().await;
}

#[tokio::test]
async fn mcp_enforces_http_security_and_stateless_methods() {
    let fixture = Fixture::start().await;
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    assert_eq!(
        client
            .post(fixture.url(crate::mcp::PATH))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    for (header, value, status) in [
        ("origin", "http://evil.test", StatusCode::FORBIDDEN),
        ("host", "evil.test", StatusCode::FORBIDDEN),
        ("mcp-protocol-version", "future", StatusCode::BAD_REQUEST),
        ("accept", "application/json", StatusCode::NOT_ACCEPTABLE),
    ] {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::HeaderName::from_bytes(header.as_bytes()).unwrap(),
            value.parse().unwrap(),
        );
        assert_eq!(
            post(
                &client,
                &fixture,
                &json!({"jsonrpc":"2.0","id":1,"method":"ping"})
            )
            .headers(headers)
            .send()
            .await
            .unwrap()
            .status(),
            status
        );
    }
    for method in [Method::GET, Method::DELETE] {
        let response = client
            .request(method, fixture.url(crate::mcp::PATH))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(response.headers()["allow"], "POST");
    }
    assert_eq!(
        post(
            &client,
            &fixture,
            &json!({"jsonrpc":"2.0","id":1,"method":"ping"})
        )
        .query(&[("token", "wrong")])
        .send()
        .await
        .unwrap()
        .status(),
        StatusCode::BAD_REQUEST
    );
    fixture.stop().await;
}
