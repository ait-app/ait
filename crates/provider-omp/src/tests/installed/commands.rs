//! Usage reads, built-in slash commands and model changes (B8) against the official peer,
//! driven by a loopback mock model.
use super::*;

/// A prompt the mock model answers only after a delay, so input can be queued behind it.
const SLOW: &str = "OMP_SLOW_FIXTURE";

/// Launch the installed peer with a mock model that reports token usage, a second model with
/// a larger window, and compaction keeping almost nothing so `/compact` has work to do.
async fn installed_with_usage_model() -> Installed {
    let fixture = installed_with_plugins().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(mock_usage_completion),
    );
    let server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    let agent = fixture.root.path().join("agent");
    let model = |id: &str, window: u64| {
        json!({"id":id,"name":id,"reasoning":false,"input":["text"],"contextWindow":window,
            "maxTokens":1024,"cost":{"input":10,"output":10,"cacheRead":1,"cacheWrite":0}})
    };
    std::fs::write(
        agent.join("models.yml"),
        json!({"providers":{"ait-local-fixture":{
        "baseUrl":format!("http://{address}/v1"),"api":"openai-completions","auth":"none",
        "models":[model("fixture", 32768), model("fixture2", 65536)]}}})
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        agent.join("config.yml"),
        "compaction:\n  keepRecentTokens: 1\n",
    )
    .unwrap();
    Installed {
        _server: server,
        ..fixture
    }
}

/// Answer every request with plain text and fixed usage: 120 prompt tokens, 20 of them cached.
async fn mock_usage_completion(
    axum::Json(request): axum::Json<Value>,
) -> ([(&'static str, &'static str); 1], String) {
    let slow = request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|message| message["role"] == "user")
        .is_some_and(|message| message["content"].to_string().contains(SLOW));
    if slow {
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    let first = json!({"id":"fixture","object":"chat.completion.chunk","created":0,
        "model":"fixture","choices":[{"index":0,"delta":{"role":"assistant","content":"plain reply"},
        "finish_reason":null}]});
    let last = json!({"id":"fixture","object":"chat.completion.chunk","created":0,
        "model":"fixture","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":120,"completion_tokens":7,"total_tokens":127,
            "prompt_tokens_details":{"cached_tokens":20}}});
    (
        [("content-type", "text/event-stream")],
        format!("data: {first}\n\ndata: {last}\n\ndata: [DONE]\n\n"),
    )
}

/// Poll `session` until an event satisfies `done`, returning every event seen.
async fn until(
    session: &mut dyn AgentSession,
    done: impl Fn(&AgentTurnEvent) -> bool,
) -> Vec<AgentTurnEvent> {
    let mut events = Vec::new();
    loop {
        let event = next(session).await.unwrap();
        let finished = done(&event);
        events.push(event);
        if finished {
            return events;
        }
    }
}

fn says(text: &str) -> impl Fn(&AgentTurnEvent) -> bool + '_ {
    move |event| matches!(event, AgentTurnEvent::Timeline(entry) if entry.item["text"] == text)
}

fn is_usage(event: &AgentTurnEvent) -> bool {
    matches!(event, AgentTurnEvent::Usage(_))
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; runs loopback-only inference"]
async fn installed_omp_session_stats_queued_messages_and_model_changes_follow_paseo() {
    let fixture = installed_with_usage_model().await;
    let config = &fixture.spec.config;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();

    session.start_turn("hello", config).await.unwrap();
    let events = complete(session.as_mut()).await;
    // OMP 18.18.8 reports uncached input, cache reads, output and cost from its stats.
    let (position, usage) = events
        .iter()
        .enumerate()
        .find_map(|(index, event)| match event {
            AgentTurnEvent::Usage(usage) => Some((index, usage)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no usage: {events:?}"));
    assert!(position < events.len() - 1);
    assert_eq!(usage.input_tokens, Some(100));
    assert_eq!(usage.cached_input_tokens, Some(20));
    assert_eq!(usage.output_tokens, Some(7));
    assert!(usage.total_cost_usd.is_some_and(|cost| cost > 0.0));
    assert_eq!(usage.context_window_max_tokens, Some(32768));
    assert_eq!(usage.context_window_used_tokens, Some(120));

    // A follow-up queued behind a slow answer runs in the same turn.
    let slow = session.start_turn(SLOW, config).await.unwrap();
    until(session.as_mut(), says(SLOW)).await;
    session
        .out_of_band(&input("/follow-up after the slow one", "c1"))
        .await
        .unwrap();
    let events = complete(session.as_mut()).await;
    assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
    let users = user_messages(&events);
    assert_eq!(users[0].item["text"], "after the slow one", "{events:?}");
    assert_eq!(users[0].item["clientMessageId"], "c1");
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentTurnEvent::Started(turn) if *turn != slow))
    );

    session
        .out_of_band(&input("/steer go on", "c2"))
        .await
        .unwrap();
    let AgentTurnEvent::Started(_) = next(session.as_mut()).await.unwrap() else {
        panic!("an idle /steer did not announce its turn");
    };
    let users = user_messages(&complete(session.as_mut()).await);
    assert_eq!(users[0].item["text"], "go on");
    assert_eq!(users[0].item["clientMessageId"], "c2");

    // OMP announces a field-less model_changed for set_model; Ait reads and publishes it.
    let larger = StoredAgentConfig {
        model: Some(json!(["ait-local-fixture", "fixture2"]).to_string()),
        ..config.clone()
    };
    session.start_turn("hello", &larger).await.unwrap();
    let events = until(session.as_mut(), |event| {
        matches!(event, AgentTurnEvent::RuntimeInfo(_))
    })
    .await;
    let Some(AgentTurnEvent::RuntimeInfo(info)) = events.last() else {
        unreachable!();
    };
    assert_eq!(info.model, larger.model);
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; runs loopback-only inference"]
async fn installed_omp_compaction_commands_follow_paseo() {
    let fixture = installed_with_usage_model().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    for text in ["hello", "hello again"] {
        session
            .start_turn(text, &fixture.spec.config)
            .await
            .unwrap();
        complete(session.as_mut()).await;
    }

    session
        .out_of_band(&AgentPrompt::text("/autocompact off"))
        .await
        .unwrap();
    until(session.as_mut(), says("Auto-compaction disabled.")).await;
    session
        .out_of_band(&AgentPrompt::text("/autocompact"))
        .await
        .unwrap();
    until(session.as_mut(), says("Auto-compaction enabled.")).await;

    session
        .out_of_band(&AgentPrompt::text("/compact keep it short"))
        .await
        .unwrap();
    let events = until(session.as_mut(), is_usage).await;
    let rows: Vec<_> = timeline_items(&events)
        .into_iter()
        .filter(|item| item["type"] == "compaction")
        .collect();
    // Only Ait's two rows: OMP's manual_compaction_start/_end are hidden.
    assert_eq!(rows.len(), 2, "{events:?}");
    assert_eq!(rows[0]["status"], "loading");
    assert_eq!(rows[1]["status"], "completed");
    assert_eq!(rows[1]["trigger"], "manual");
    assert!(
        rows[1]["preTokens"]
            .as_u64()
            .is_some_and(|tokens| tokens > 0)
    );
    assert_eq!(rows[0]["compactionId"], rows[1]["compactionId"]);

    // OMP 18.18.8 has nothing left to hand off right after a compaction.
    session
        .out_of_band(&AgentPrompt::text("/handoff"))
        .await
        .unwrap();
    until(
        session.as_mut(),
        says("[Error] Failed to hand off turn: Nothing to hand off (already compacted)"),
    )
    .await;
    session.close().await.unwrap();
}
