//! Native BTW must retain an inert tool catalog without entering the agent execution loop.
use super::*;
use provider_core::ports::side_questions::{SideAsk, SideRunnerSpec};
use provider_core::protocol::side_questions::{
    SideContextEntry, SideContextSnapshot, SideTurnStatus,
};
use std::sync::{Arc, Mutex};

type Captures = Arc<Mutex<Vec<Value>>>;

async fn tool_call_model(
    axum::extract::State(captures): axum::extract::State<Captures>,
    axum::Json(request): axum::Json<Value>,
) -> ([(&'static str, &'static str); 1], String) {
    captures.lock().unwrap().push(request);
    let arguments = json!({"command":"printf EXECUTED > btw-tool-executed"}).to_string();
    let first = json!({"id":"btw-fixture","object":"chat.completion.chunk","created":0,
        "model":"fixture","choices":[{"index":0,"delta":{"role":"assistant",
        "content":"Native side answer", "tool_calls":[{"index":0,"id":"malicious-call",
        "type":"function","function":{"name":"bash","arguments":arguments}}]},
        "finish_reason":null}]});
    let last = json!({"id":"btw-fixture","object":"chat.completion.chunk","created":0,
        "model":"fixture","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],
        "usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}});
    (
        [("content-type", "text/event-stream")],
        format!("data: {first}\n\ndata: {last}\n\ndata: [DONE]\n\n"),
    )
}

async fn installed_with_tool_call() -> (Installed, Captures) {
    let mut fixture = installed_with_plugins().await;
    let mut environment: BTreeMap<String, String> = fixture
        .client
        .environment
        .entries()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    environment.insert("HOME".into(), fixture.root.path().to_str().unwrap().into());
    fixture.client.environment = AgentEnvironment::try_from(environment).unwrap();
    let captures = Captures::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = axum::Router::new()
        .route("/v1/chat/completions", axum::routing::post(tool_call_model))
        .with_state(captures.clone());
    let server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    std::fs::write(
        fixture.root.path().join("agent/models.yml"),
        json!({"providers":{"ait-local-fixture":{
            "baseUrl":format!("http://{address}/v1"),"api":"openai-completions","auth":"none",
            "models":[{"id":"fixture","name":"Local fixture","reasoning":false,
                "input":["text"],"contextWindow":32768,"maxTokens":1024,
                "cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0}}]}}})
        .to_string(),
    )
    .unwrap();
    (
        Installed {
            _server: server,
            ..fixture
        },
        captures,
    )
}

#[tokio::test]
#[ignore = "requires AIT_TEST_OMP_BIN; uses only a loopback adversarial model fixture"]
async fn installed_native_btw_discards_model_tool_calls_without_execution_or_main_history() {
    let (fixture, captures) = installed_with_tool_call().await;
    let mut main = fixture.client.create_session(&fixture.spec).await.unwrap();
    let handle = main.persistence().unwrap();
    let spec = SideRunnerSpec {
        session: fixture.spec.clone(),
        environment: AgentEnvironment::default(),
        directory: fixture.root.path().join("aux"),
    };
    let mut side = fixture.client.create_side_questions(&spec).await.unwrap();
    assert!(captures.lock().unwrap().is_empty());
    side.side_ask(SideAsk {
        request_id: "native-tool-call-probe".into(),
        question: "Explain the design".into(),
        record_id: None,
        previous: None,
        snapshot: SideContextSnapshot {
            v: 1,
            agent_id: "main".into(),
            provider: "omp".into(),
            model: spec.session.config.model.clone(),
            captured_at: 1,
            native_session_id: None,
            native_leaf_id: None,
            entries: vec![SideContextEntry {
                role: "user".into(),
                text: "Frozen source context".into(),
            }],
            truncated: false,
        },
    })
    .await
    .unwrap();
    let topic = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let history = side.side_history().await.unwrap();
            if let Some(topic) = history
                .first()
                .filter(|t| t.latest_turn().status.is_terminal())
            {
                break topic.clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(topic.record.status, SideTurnStatus::Complete, "{topic:?}");
    assert_eq!(topic.record.answer, "Native side answer");
    side.close_side().await.unwrap();
    main.close().await.unwrap();
    assert!(!fixture.root.path().join("btw-tool-executed").exists());
    assert!(
        fixture
            .client
            .history(&handle, &fixture.spec.cwd)
            .await
            .unwrap()
            .is_empty()
    );
    let requests = captures.lock().unwrap();
    assert_eq!(
        requests.len(),
        1,
        "tool calls must not cause a second agent iteration"
    );
    assert_eq!(requests[0]["model"], "fixture");
    assert!(
        requests[0]["tools"]
            .as_array()
            .is_some_and(|tools| !tools.is_empty())
    );
    assert!(
        requests[0]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["role"] != "tool")
    );
}
