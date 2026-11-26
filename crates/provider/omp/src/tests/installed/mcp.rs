use super::*;

#[tokio::test]
#[ignore = "requires AIT_TEST_OMP_BIN; public host tools call an isolated Ait MCP fixture"]
async fn installed_omp_calls_ait_to_delegate_to_another_provider() {
    let mut fixture = installed_with_plugins().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (sent, mut received) = tokio::sync::mpsc::unbounded_channel();
    let router = axum::Router::new()
        .route("/v1/chat/completions", axum::routing::post(completion))
        .route("/mcp", axum::routing::post(move |headers: axum::http::HeaderMap, axum::Json(request): axum::Json<Value>| {
            let sent = sent.clone();
            async move {
                assert_eq!(headers["x-ait-agent-id"], "parent-fixture");
                let result = match request["method"].as_str().unwrap() {
                    "initialize" => json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{}}}),
                    "tools/list" => json!({"tools":[{"name":"agent_create","description":"Create an Ait child agent","inputSchema":{"type":"object","properties":{"provider":{"type":"string"},"initialPrompt":{"type":"string"}},"required":["provider","initialPrompt"]}}]}),
                    "tools/call" => { sent.send(request["params"].clone()).unwrap(); json!({"content":[{"type":"text","text":"Child admitted"}]}) },
                    _ => panic!("unexpected MCP method"),
                };
                axum::Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result}))
            }
        }));
    let _server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    let path = fixture.root.path().join("agent/models.yml");
    let mut models: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    models["providers"]["ait-local-fixture"]["baseUrl"] = json!(format!("http://{address}/v1"));
    std::fs::write(path, models.to_string()).unwrap();
    fixture.spec.config.mcp_servers = Some(BTreeMap::from([(
        "ait".into(),
        json!({"type":"http","url":format!("http://{address}/mcp"),"headers":{"X-Ait-Agent-Id":"parent-fixture"}}),
    )]));
    assert!(fixture.client.supports_agent_mcp());
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn("Delegate a UI review", &fixture.spec.config)
        .await
        .unwrap();
    let events = complete(session.as_mut()).await;
    assert!(
        matches!(events.last(), Some(AgentTurnEvent::Completed(Some(answer))) if answer == "Delegated")
    );
    let request = received.try_recv().unwrap();
    assert_eq!(request["name"], "agent_create");
    assert_eq!(request["arguments"]["provider"], "antigravity");
    // Native mode transitions must preserve the orchestration tool catalog.
    commands_ready(session.as_mut()).await;
    session
        .out_of_band(&AgentPrompt::text("/vibe on"))
        .await
        .unwrap();
    session
        .out_of_band(&AgentPrompt::text("/vibe off"))
        .await
        .unwrap();
    session
        .start_turn("Delegate another review", &fixture.spec.config)
        .await
        .unwrap();
    let events = complete(session.as_mut()).await;
    assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
    session.close().await.unwrap();
}

async fn completion(
    axum::Json(request): axum::Json<Value>,
) -> ([(&'static str, &'static str); 1], String) {
    if let Some(tools) = request["tools"].as_array() {
        assert!(
            tools
                .iter()
                .any(|tool| tool["function"]["name"] == "mcp__ait__agent_create")
        );
    }
    let called = request["tools"].is_null()
        || request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "tool");
    let delta = if called {
        json!({"role":"assistant","content":"Delegated"})
    } else {
        json!({"role":"assistant","tool_calls":[{"index":0,"id":"delegate","type":"function","function":{"name":"mcp__ait__agent_create","arguments":json!({"provider":"antigravity","initialPrompt":"Review the toggle UI"}).to_string()}}]})
    };
    let first = json!({"id":"fixture","object":"chat.completion.chunk","created":0,"model":"fixture","choices":[{"index":0,"delta":delta,"finish_reason":null}]});
    let last = json!({"id":"fixture","object":"chat.completion.chunk","created":0,"model":"fixture","choices":[{"index":0,"delta":{},"finish_reason":if called {"stop"} else {"tool_calls"}}],"usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}});
    (
        [("content-type", "text/event-stream")],
        format!("data: {first}\n\ndata: {last}\n\ndata: [DONE]\n\n"),
    )
}
