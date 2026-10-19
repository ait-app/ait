//! Native placeholder counters must not make a populated session look empty.
use super::*;

#[tokio::test]
#[ignore = "requires OpenCode 1.18.4; isolated XDG and loopback model"]
async fn installed_acp_cancelled_placeholder_does_not_report_empty_context() {
    let mut fixture = installed(false).await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn("Seed usage", &fixture.spec.config)
        .await
        .unwrap();
    let mut usage = None;
    loop {
        match event(session.as_mut()).await {
            AgentTurnEvent::Usage(snapshot) => usage = Some(snapshot),
            AgentTurnEvent::Completed(_) => break,
            AgentTurnEvent::Failed | AgentTurnEvent::Cancelled => panic!("seed failed"),
            _ => {}
        }
    }
    let snapshot = usage.as_ref().unwrap();
    assert_eq!(snapshot.context_window_used_tokens, Some(10));
    assert_eq!(snapshot.context_window_max_tokens, Some(32_000));
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    session = fixture
        .client
        .resume_session(&handle, &fixture.spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert!(
        matches!(event(session.as_mut()).await, AgentTurnEvent::Usage(snapshot)
        if snapshot.context_window_used_tokens == Some(10))
    );
    let turn = session
        .start_turn("Hold native request", &fixture.spec.config)
        .await
        .unwrap();
    workflows::request(&mut fixture, "Hold native request").await;
    session.cancel_turn(&turn).await.unwrap();
    loop {
        match event(session.as_mut()).await {
            AgentTurnEvent::Usage(snapshot) => usage = Some(snapshot),
            AgentTurnEvent::Cancelled => break,
            AgentTurnEvent::Completed(_) | AgentTurnEvent::Failed => panic!("cancel not confirmed"),
            _ => {}
        }
    }
    let snapshot = usage.unwrap();
    assert_eq!(snapshot.context_window_used_tokens, Some(10));
    assert_eq!(snapshot.context_window_max_tokens, Some(32_000));
    session
        .start_turn("Continue", &fixture.spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires OpenCode 1.18.4; ACP execution and native live metadata on isolated loopback"]
async fn installed_legacy_acp_reports_real_usage_while_the_tool_turn_is_still_running() {
    let mut fixture = installed(false).await;
    fixture.spec.config.feature_values =
        Some(BTreeMap::from([("permission".into(), json!("allow"))]));
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    let turn = session
        .start_turn("Observe native usage during tools", &fixture.spec.config)
        .await
        .unwrap();
    workflows::request(&mut fixture, "Observe native usage during tools").await;
    loop {
        match event(session.as_mut()).await {
            AgentTurnEvent::Usage(usage) if usage.context_window_used_tokens == Some(12_345) => {
                break;
            }
            AgentTurnEvent::Completed(_) | AgentTurnEvent::Cancelled | AgentTurnEvent::Failed => {
                panic!("usage must be reported before the native turn ends");
            }
            _ => {}
        }
    }
    workflows::request(&mut fixture, "Observe native usage during tools").await;
    session.cancel_turn(&turn).await.unwrap();
    loop {
        match event(session.as_mut()).await {
            AgentTurnEvent::Cancelled => break,
            AgentTurnEvent::Completed(_) | AgentTurnEvent::Failed => panic!("cancel not confirmed"),
            _ => {}
        }
    }
    session
        .start_turn("Continue", &fixture.spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires OpenCode 1.18.4; native unknown finish must not become a successful ACP turn"]
async fn installed_legacy_acp_exposes_unknown_model_finish_and_preserves_user_continuation() {
    let fixture = installed(false).await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn("Unknown native finish", &fixture.spec.config)
        .await
        .unwrap();
    loop {
        match event(session.as_mut()).await {
            AgentTurnEvent::Failed => break,
            AgentTurnEvent::Completed(_) | AgentTurnEvent::Cancelled => {
                panic!("unknown finish hidden")
            }
            _ => {}
        }
    }
    assert!(
        session
            .failure_message()
            .unwrap()
            .contains("without a valid finish reason")
    );
    session
        .start_turn("Continue", &fixture.spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    session.close().await.unwrap();
}

/// Delay the next native model step after one real tool result, without imposing a host timeout.
pub(super) async fn model_response(request: &Value) -> Option<Response> {
    if let Some(finish) = [
        ("Unknown native finish", "unknown"),
        ("Native output limit", "length"),
        ("Native refusal", "content_filter"),
    ]
    .into_iter()
    .find_map(|(prompt, finish)| is_user_message(request, prompt).then_some(finish))
    {
        let chunks = [
            json!({"choices":[{"index":0,"delta":{"role":"assistant","content":"partial answer"},"finish_reason":null}]}),
            json!({"choices":[{"index":0,"delta":{},"finish_reason":finish}],"usage":{"prompt_tokens":20,"completion_tokens":5,"total_tokens":25}}),
        ];
        let body = format!("data: {}\n\ndata: {}\n\n", chunks[0], chunks[1]);
        return Some(
            (
                [("content-type", "text/event-stream")],
                format!("{body}data: [DONE]\n\n"),
            )
                .into_response(),
        );
    }
    if !is_user_message(request, "Observe native usage during tools") {
        return None;
    }
    if request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|message| message["role"] == "tool")
    {
        return Some(std::future::pending().await);
    }
    let tool = if request["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["function"]["name"] == "bash")
    {
        "bash"
    } else {
        "shell"
    };
    Some(streamed_response_with_usage(
        &json!({"role":"assistant","tool_calls":[{
            "index":0,"id":"live-usage-tool","type":"function","function":{"name":tool,
                "arguments":json!({"command":"pwd","description":"Check native live usage"}).to_string()}
        }]}),
        true,
        12_345,
    ))
}
