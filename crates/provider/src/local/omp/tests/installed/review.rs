use super::*;

/// End the native goal through its real tool; only the model response is simulated.
pub(super) fn completion(request: &Value) -> Option<Value> {
    (request["messages"]
        .to_string()
        .contains("AIT_REVIEW_GOAL_COMPLETE")
        && request["tools"]
            .as_array()
            .is_some_and(|tools| tools.iter().any(|tool| tool["function"]["name"] == "goal")))
    .then(|| {
        json!({"role":"assistant","tool_calls":[{"index":0,"id":"finish-goal","type":"function",
            "function":{"name":"goal","arguments":"{\"op\":\"complete\"}"}}]})
    })
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; verifies completed goals with a loopback model"]
async fn installed_omp_completed_goal_clears_state_and_allows_another_goal_and_vibe() {
    let fixture = installed_with_plugins().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    for _ in 0..2 {
        session
            .start_turn("/goal AIT_REVIEW_GOAL_COMPLETE", &fixture.spec.config)
            .await
            .unwrap();
        let events = tokio::time::timeout(Duration::from_secs(30), complete(session.as_mut()))
            .await
            .unwrap();
        assert!(
            matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
            "{events:?}"
        );
        let result = action(session.as_mut(), "goal", json!({"op":"get"})).await;
        assert!(
            result["goal"].is_null() && result["state"].is_null(),
            "{result}"
        );
        session
            .out_of_band(&AgentPrompt::text("/vibe"))
            .await
            .unwrap();
        assert_eq!(session.ui_state().unwrap()["vibe"]["enabled"], true);
        session
            .out_of_band(&AgentPrompt::text("/vibe"))
            .await
            .unwrap();
    }
    let handle = session.persistence().unwrap();
    let journal =
        std::fs::read_to_string(handle.metadata.unwrap()["sessionFile"].as_str().unwrap()).unwrap();
    assert_eq!(
        journal
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|entry| entry["customType"] == "goal-completed")
            .count(),
        2
    );
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; holds a loopback model response to verify native BTW admission"]
async fn installed_omp_rejects_overlapping_btw_and_keeps_the_first_topic_cancellable() {
    let root = tempfile::tempdir().unwrap();
    let native = root.path().join("agent");
    std::fs::create_dir_all(native.join("extensions")).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(async || std::future::pending::<String>().await),
    );
    let _server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    model_roles::native_profile(&native, &format!("http://{address}/v1"));
    let client = actions::installed_client(&native);
    let mut transport = session::probe(&client, root.path().to_str().unwrap(), None)
        .await
        .unwrap();
    let first = transport
        .request("btw", json!({"question":"hold first question"}))
        .await
        .unwrap();
    let history = transport
        .request("get_btw_history", json!({}))
        .await
        .unwrap();
    let id = history["records"][0]["id"].as_str().unwrap().to_owned();
    assert_eq!(
        history["records"][0]["status"], "running",
        "{first}: {history}"
    );
    assert_eq!(
        transport
            .request("btw", json!({"question":"second question"}))
            .await,
        Err(AgentSessionError::Rejected)
    );
    let history = transport
        .request("get_btw_history", json!({}))
        .await
        .unwrap();
    assert_eq!(history["records"].as_array().unwrap().len(), 1);
    assert_eq!(history["records"][0]["id"], id);
    assert_eq!(history["records"][0]["status"], "running");
    transport
        .request("btw_cancel", json!({"recordId":id}))
        .await
        .unwrap();
    let history = transport
        .request("get_btw_history", json!({}))
        .await
        .unwrap();
    assert_eq!(history["records"][0]["status"], "cancelled");
    transport.close().await.unwrap();
}

async fn action(session: &mut dyn AgentSession, method: &str, fields: Value) -> Value {
    let mut receipt = session.begin_action(method, &fields).await.unwrap();
    let value = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            tokio::select! {
                value = &mut receipt => break value.unwrap(),
                () = tokio::time::sleep(Duration::from_millis(10)) => while session.poll_turn().unwrap().is_some() {},
            }
        }
    }).await.unwrap();
    assert!(value["error"].is_null(), "{value}");
    serde_json::from_str(value["text"].as_str().unwrap()).unwrap()
}
