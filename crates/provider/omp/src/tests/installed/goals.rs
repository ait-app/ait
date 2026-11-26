use super::*;

#[tokio::test]
#[ignore = "requires AIT_TEST_OMP_BIN; exercises native Goal RPC with isolated loopback configuration"]
async fn installed_omp_goal_rpc_and_restoration_do_not_require_a_slash_completion() {
    let fixture = installed_with_plugins().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    // OMP writes the native session file after its first assistant response.
    session
        .start_turn("Initialize isolated session", &fixture.spec.config)
        .await
        .unwrap();
    complete(session.as_mut()).await;
    while session.command_catalog().is_none() {
        next(session.as_mut()).await.unwrap();
    }
    assert!(
        session
            .command_catalog()
            .unwrap()
            .iter()
            .any(|entry| entry["name"] == "goal")
    );
    assert_eq!(
        session.runtime_info().await.unwrap().extra.unwrap()["goal"],
        Value::Null
    );
    for (command, status) in [
        ("/goal Verify isolated native Goal RPC", "active"),
        ("/goal pause", "paused"),
    ] {
        session
            .out_of_band(&AgentPrompt::text(command))
            .await
            .unwrap();
        wait_goal(session.as_mut(), |goal| goal["status"] == status).await;
    }
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let mut restored = fixture
        .client
        .resume_session(&handle, &fixture.spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    let goal = restored.runtime_info().await.unwrap().extra.unwrap()["goal"].clone();
    assert_eq!(goal["objective"], "Verify isolated native Goal RPC");
    assert_eq!(goal["status"], "paused");
    while restored.command_catalog().is_none() {
        next(restored.as_mut()).await.unwrap();
    }
    for (command, status) in [("/goal resume", "active"), ("/goal pause", "paused")] {
        restored
            .out_of_band(&AgentPrompt::text(command))
            .await
            .unwrap();
        wait_goal(restored.as_mut(), |goal| goal["status"] == status).await;
    }
    restored
        .out_of_band(&AgentPrompt::text("/goal clear"))
        .await
        .unwrap();
    wait_goal(restored.as_mut(), Value::is_null).await;
    restored.close().await.unwrap();
}

async fn wait_goal(session: &mut dyn AgentSession, expected: impl Fn(&Value) -> bool) {
    loop {
        let event = next(session).await.unwrap();
        if let AgentTurnEvent::RuntimeInfo(info) = event
            && let Some(goal) = info.extra.as_ref().and_then(|extra| extra.get("goal"))
            && expected(goal)
        {
            return;
        }
    }
}

#[tokio::test]
#[ignore = "requires AIT_TEST_OMP_BIN; runs native Goal continuation and completion against loopback"]
async fn installed_omp_initial_goal_continues_until_the_native_goal_tool_completes_it() {
    let fixture = installed_with_plugins().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let received = requests.clone();
    let router = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(move |axum::Json(request): axum::Json<Value>| {
            let received = received.clone();
            async move {
                if !request["tools"].is_array() {
                    return goal_completion(3);
                }
                let mut requests = received.lock().unwrap();
                let step = requests.len();
                requests.push(request);
                goal_completion(step)
            }
        }),
    );
    let _server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    model_endpoint(&fixture, address);
    std::fs::write(
        fixture.root.path().join("evidence.txt"),
        "Acceptance evidence",
    )
    .unwrap();
    let (execution, _) = provider::testing::worker_at(
        fixture.root.path(),
        std::path::Path::new(&fixture.spec.cwd),
        model::creation::Creations::default(),
        Box::new(fixture.client.clone()),
    );
    let created = execution.execute("agent.create.request", json!({
        "config":{"provider":"omp","cwd":fixture.spec.cwd,"model":fixture.spec.config.model},
        "initialPrompt":"/goal Read evidence.txt, verify it on a second turn, then complete the goal.",
        "clientMessageId":"native-initial-goal"
    })).await.unwrap();
    let finished = execution
        .execute(
            "agent.finish.wait.request",
            json!({
                "agentId":created["agentId"],"timeoutMs":20000
            }),
        )
        .await
        .unwrap();
    assert_eq!(
        finished["lastMessage"], "Verified goal complete.",
        "{finished}"
    );
    execution.shutdown().await.unwrap();
    let requests = requests.lock().unwrap();
    assert_eq!(
        requests.len(),
        4,
        "native continuation must run a second turn"
    );
    assert!(
        requests[0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["function"]["name"] == "goal")
    );
    assert!(
        requests[2]["messages"]
            .to_string()
            .contains("First pass recorded.")
    );
    assert!(!requests[0]["messages"].to_string().contains("/goal Read"));
}

fn goal_completion(step: usize) -> ([(&'static str, &'static str); 1], String) {
    let (delta, reason) = match step {
        0 | 2 => {
            let (name, args) = if step == 0 {
                ("read", json!({"path":"evidence.txt"}))
            } else {
                ("goal", json!({"op":"complete"}))
            };
            (
                json!({"role":"assistant","tool_calls":[{"index":0,"id":format!("goal-step-{step}"),"type":"function",
                "function":{"name":name,"arguments":args.to_string()}}]}),
                "tool_calls",
            )
        }
        1 => (
            json!({"role":"assistant","content":"First pass recorded."}),
            "stop",
        ),
        _ => (
            json!({"role":"assistant","content":"Verified goal complete."}),
            "stop",
        ),
    };
    let first = json!({"id":format!("goal-response-{step}"),"object":"chat.completion.chunk","created":0,"model":"fixture",
        "choices":[{"index":0,"delta":delta,"finish_reason":null}]});
    let last = json!({"id":format!("goal-response-{step}"),"object":"chat.completion.chunk","created":0,"model":"fixture",
        "choices":[{"index":0,"delta":{},"finish_reason":reason}],
        "usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}});
    (
        [("content-type", "text/event-stream")],
        format!("data: {first}\n\ndata: {last}\n\ndata: [DONE]\n\n"),
    )
}

fn model_endpoint(fixture: &Installed, address: std::net::SocketAddr) {
    let path = fixture.root.path().join("agent/models.yml");
    let mut models: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    models["providers"]["ait-local-fixture"]["baseUrl"] = json!(format!("http://{address}/v1"));
    std::fs::write(path, models.to_string()).unwrap();
}

#[tokio::test]
#[ignore = "requires AIT_TEST_OMP_BIN; stops a native Goal before its first reported turn"]
async fn installed_omp_pending_goal_stop_is_persisted_before_any_user_prompt() {
    let fixture = installed_with_plugins().await;
    // A fast model response must not accidentally satisfy the native lazy persistence gate.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    model_endpoint(&fixture, listener.local_addr().unwrap());
    let router = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(std::future::pending::<&'static str>),
    );
    let _server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    commands_ready(session.as_mut()).await;
    session
        .out_of_band(&AgentPrompt::text("/goal Preserve this stopped objective"))
        .await
        .unwrap();
    assert!(session.pending_foreground());
    session.cancel_pending().await.unwrap();
    let goal = session.runtime_info().await.unwrap().extra.unwrap()["goal"].clone();
    assert_eq!(goal["status"], "paused");
    let handle = session.persistence().unwrap();
    assert!(
        std::path::Path::new(
            handle.metadata.as_ref().unwrap()["sessionFile"]
                .as_str()
                .unwrap()
        )
        .is_file()
    );
    session.close().await.unwrap();
    let mut restored = fixture
        .client
        .resume_session(&handle, &fixture.spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    let goal = restored.runtime_info().await.unwrap().extra.unwrap()["goal"].clone();
    assert_eq!(goal["status"], "paused");
    assert_eq!(goal["objective"], "Preserve this stopped objective");
    restored.close().await.unwrap();
}
