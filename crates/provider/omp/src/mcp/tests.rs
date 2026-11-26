use super::*;

#[test]
fn scoped_http_configuration_rejects_unrelated_servers_and_embedded_credentials() {
    for server in [
        json!({"type":"stdio","command":"unsafe"}),
        json!({"type":"http","url":"http://user:password@localhost"}),
        json!({"type":"http","url":"file:///tmp/socket"}),
    ] {
        assert!(!valid(&StoredAgentConfig {
            mcp_servers: Some(BTreeMap::from([("ait".into(), server)])),
            ..Default::default()
        }));
    }
    let config = StoredAgentConfig {
        mcp_servers: Some(BTreeMap::from([(
            "ait".into(),
            json!({"type":"http","url":"http://127.0.0.1:1234/mcp","headers":{"X-Ait-Agent-Id":"parent"}}),
        )])),
        ..Default::default()
    };
    assert!(valid(&config));
    let (_, headers) = connection(&config.mcp_servers.unwrap()["ait"]).unwrap();
    assert!(headers["x-ait-agent-id"].is_sensitive());
}

#[tokio::test]
async fn host_tools_forward_cross_provider_arguments_with_scoped_identity() {
    let (_root, client, mut spec) = crate::tests::fixture();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (sent, mut received) = tokio::sync::mpsc::unbounded_channel();
    let router = axum::Router::new().route("/mcp", axum::routing::post(move |headers: HeaderMap, axum::Json(request): axum::Json<Value>| {
        let sent = sent.clone();
        async move {
            assert_eq!(headers["x-ait-agent-id"], "parent");
            assert_eq!(headers["authorization"], "Bearer fixture-only");
            let result = match request["method"].as_str().unwrap() {
                "initialize" => json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{}}}),
                "tools/list" => json!({"tools":[{"name":"agent_create","description":"Create a scoped agent","inputSchema":{"type":"object"}}]}),
                "tools/call" => { sent.send(request["params"].clone()).unwrap(); json!({"content":[{"type":"text","text":"admitted"}],"isError":false}) },
                _ => panic!("unexpected MCP method"),
            };
            axum::Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result}))
        }
    }));
    let _server = AbortOnDropHandle::new(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    spec.config.mcp_servers = Some(BTreeMap::from([(
        "ait".into(),
        json!({"type":"http","url":format!("http://{address}/mcp"),"headers":{"X-Ait-Agent-Id":"parent","Authorization":"Bearer fixture-only"}}),
    )]));
    let mut transport = crate::session::probe(&client, &spec.cwd, client.timeouts)
        .await
        .unwrap();
    let mut mcp = Mcp::open(&spec.config, &mut transport)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(mcp.definitions()[0]["name"], "mcp__ait__agent_create");
    let arguments = json!({"provider":"antigravity","model":"gemini-fixture","title":"UI review","initialPrompt":"Review the toggles"});
    assert!(mcp.consume(&json!({"type":"host_tool_call","id":"create-child","toolName":"mcp__ait__agent_create","arguments":arguments}),transport.writer().unwrap()).unwrap());
    let request = tokio::time::timeout(Duration::from_secs(5), received.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        request,
        json!({"name":"agent_create","arguments":arguments})
    );
    assert!(
        !mcp.consume(
            &json!({"type":"host_tool_call","id":"foreign","toolName":"mcp__other__agent_create"}),
            transport.writer().unwrap()
        )
        .unwrap()
    );
    assert!(
        mcp.consume(
            &json!({"type":"host_tool_cancel","targetId":"create-child"}),
            transport.writer().unwrap()
        )
        .unwrap()
    );
    transport.close().await.unwrap();
}
