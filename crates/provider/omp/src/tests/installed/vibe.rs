use super::*;

#[tokio::test]
#[ignore = "requires AIT_TEST_OMP_BIN; public Vibe extension with isolated loopback configuration"]
async fn installed_omp_vibe_toggle_is_confirmed_and_mutually_exclusive_with_goal() {
    let fixture = installed_with_plugins().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    while session.command_catalog().is_none() {
        next(session.as_mut()).await.unwrap();
    }
    assert!(
        session
            .command_catalog()
            .unwrap()
            .iter()
            .any(|entry| entry["name"] == "vibe")
    );
    session
        .out_of_band(&AgentPrompt::text("/vibe on"))
        .await
        .unwrap();
    assert_eq!(
        session.runtime_info().await.unwrap().extra.unwrap()["vibe"]["enabled"],
        true
    );
    assert!(
        session
            .out_of_band(&AgentPrompt::text("/goal conflicting goal"))
            .await
            .is_err()
    );
    session
        .out_of_band(&AgentPrompt::text("/vibe off"))
        .await
        .unwrap();
    assert_eq!(
        session.runtime_info().await.unwrap().extra.unwrap()["vibe"]["enabled"],
        false
    );
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires AIT_TEST_OMP_BIN; persistent Vibe workers against a loopback-only model"]
async fn installed_omp_vibe_workers_publish_children_and_restore_without_auto_enabling() {
    let fixture = installed_with_plugins().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router =
        axum::Router::new().route("/v1/chat/completions", axum::routing::post(vibe_completion));
    let _server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    let path = fixture.root.path().join("agent/models.yml");
    let mut models: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    models["providers"]["ait-local-fixture"]["baseUrl"] = json!(format!("http://{address}/v1"));
    std::fs::write(path, models.to_string()).unwrap();
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    while session.command_catalog().is_none() {
        next(session.as_mut()).await.unwrap();
    }
    session
        .start_turn("/vibe VIBE_DIRECTOR_FIXTURE", &fixture.spec.config)
        .await
        .unwrap();
    let mut completed_runs = 0;
    let mut worker_completed = false;
    while !worker_completed || completed_runs < 2 {
        let event = next_within(session.as_mut(), Duration::from_secs(60))
            .await
            .unwrap();
        match event {
            AgentTurnEvent::Completed(_) => completed_runs += 1,
            AgentTurnEvent::Subagent(provider_core::ports::controls::SubagentEvent::Upsert(
                child,
            )) => {
                worker_completed |= child.descriptor["status"] == "completed";
            }
            AgentTurnEvent::Failed => panic!("Vibe director or continuation failed"),
            _ => {}
        }
    }
    let children = session.subagents();
    assert_eq!(children.len(), 1);
    let handle = session.persistence().unwrap();
    assert_eq!(children[0].parent_id, handle.session_id);
    assert_eq!(children[0].descriptor["status"], "completed");
    assert!(children[0].persistence.is_some());
    let receipt = session.begin_child_stop(&children[0].id).unwrap();
    loop {
        if let AgentTurnEvent::ChildControl { token, outcome } =
            next_within(session.as_mut(), Duration::from_secs(60))
                .await
                .unwrap()
        {
            assert_eq!(token, receipt.token);
            assert!(matches!(
                outcome,
                provider_core::ports::controls::ChildControlOutcome::Stop(Ok(
                    provider_core::ports::controls::ChildStopOutcome::NotRunning
                ))
            ));
            break;
        }
    }
    let history = fixture
        .client
        .history(children[0].persistence.as_ref().unwrap(), &fixture.spec.cwd)
        .await
        .unwrap();
    assert!(
        history
            .iter()
            .any(|entry| entry.item.to_string().contains("Worker finished"))
    );
    session
        .out_of_band(&AgentPrompt::text("/vibe off"))
        .await
        .unwrap();
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let mut restored = fixture
        .client
        .resume_session(&handle, &fixture.spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert_eq!(restored.subagents().len(), 1);
    assert_eq!(
        restored.runtime_info().await.unwrap().extra.unwrap()["vibe"]["enabled"],
        false
    );
    restored.close().await.unwrap();
}

async fn vibe_completion(
    axum::Json(request): axum::Json<Value>,
) -> ([(&'static str, &'static str); 1], String) {
    let messages = request["messages"].as_array().unwrap();
    let director = messages.iter().any(|message| {
        message["role"] == "user"
            && message["content"]
                .to_string()
                .contains("VIBE_DIRECTOR_FIXTURE")
    });
    let called = messages
        .iter()
        .any(|message| message["role"] == "assistant" && message["tool_calls"].is_array());
    let delta = if director && !called {
        let names: Vec<_> = request["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["function"]["name"].as_str())
            .collect();
        assert!(names.contains(&"vibe_spawn"), "{names:?}");
        assert!(
            !names.contains(&"bash") && !names.contains(&"edit"),
            "director must be read-only: {names:?}"
        );
        json!({"role":"assistant","tool_calls":[{"index":0,"id":"spawn-worker","type":"function","function":{"name":"vibe_spawn","arguments":json!({"cli":"fast","name":"Worker","prompt":"VIBE_WORKER_FIXTURE"}).to_string()}}]})
    } else {
        json!({"role":"assistant","content":if director {"Director done"} else {"Worker finished"}})
    };
    let finish = if delta["tool_calls"].is_array() {
        "tool_calls"
    } else {
        "stop"
    };
    let first = json!({"id":"fixture","object":"chat.completion.chunk","created":0,"model":"fixture","choices":[{"index":0,"delta":delta,"finish_reason":null}]});
    let last = json!({"id":"fixture","object":"chat.completion.chunk","created":0,"model":"fixture","choices":[{"index":0,"delta":{},"finish_reason":finish}],"usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}});
    (
        [("content-type", "text/event-stream")],
        format!("data: {first}\n\ndata: {last}\n\ndata: [DONE]\n\n"),
    )
}
