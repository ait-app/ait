//! ACP-only acceptance of native 2.x stop reasons and request-scoped context accounting.
use super::*;

#[tokio::test]
#[ignore = "requires OpenCode 2.0.25+; native output-limit recovery, refusal and explicit continuation"]
async fn installed_acp_v2_output_limits_and_refusal_preserve_native_outcomes() {
    let fixture = installed(false).await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn("Native output limit", &fixture.spec.config)
        .await
        .unwrap();
    // 2.0.32 continues length natively; 2.0.25 returns max_tokens directly.
    let mut history = Vec::new();
    let limited = loop {
        match event(session.as_mut()).await {
            AgentTurnEvent::History(entries) => history = entries,
            AgentTurnEvent::Completed(_) => break false,
            AgentTurnEvent::Failed => break true,
            AgentTurnEvent::Cancelled => panic!("native limit became cancellation"),
            _ => {}
        }
    };
    let replies: Vec<_> = history
        .iter()
        .filter(|entry| entry.item["type"] == "assistant_message")
        .map(|entry| entry.item["text"].as_str().unwrap())
        .collect();
    if limited {
        assert_eq!(replies, ["partial answer"]);
        assert!(session.failure_message().unwrap().contains("output limit"));
    } else {
        assert_eq!(replies, ["partial answer", "authoritative answer"]);
        assert_eq!(session.failure_message(), None);
    }
    session
        .start_turn("Continue", &fixture.spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    assert_eq!(session.failure_message(), None);
    session
        .start_turn("Native refusal", &fixture.spec.config)
        .await
        .unwrap();
    loop {
        match event(session.as_mut()).await {
            AgentTurnEvent::Failed => break,
            AgentTurnEvent::Completed(_) | AgentTurnEvent::Cancelled => {
                panic!("native refusal hidden")
            }
            _ => {}
        }
    }
    assert!(session.failure_message().unwrap().contains("refused"));
    session
        .start_turn("Continue", &fixture.spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    assert_eq!(session.failure_message(), None);
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires OpenCode 2.0.25+; native last-step usage on cancellation and explicit continuation"]
async fn installed_acp_v2_cancel_reports_the_last_real_request_and_can_continue() {
    let mut fixture = installed(false).await;
    fixture.spec.config.feature_values =
        Some(BTreeMap::from([("permission".into(), json!("allow"))]));
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    let turn = session
        .start_turn("Observe native usage during tools", &fixture.spec.config)
        .await
        .unwrap();
    workflows::request(&mut fixture, "Observe native usage during tools").await;
    workflows::request(&mut fixture, "Observe native usage during tools").await;
    session.cancel_turn(&turn).await.unwrap();
    let mut usage = None;
    loop {
        match event(session.as_mut()).await {
            AgentTurnEvent::Usage(snapshot) => usage = Some(snapshot),
            AgentTurnEvent::Cancelled => break,
            AgentTurnEvent::Completed(_) | AgentTurnEvent::Failed => {
                panic!("native cancel not confirmed")
            }
            _ => {}
        }
    }
    let usage = usage.unwrap();
    assert_eq!(usage.context_window_used_tokens, Some(12_350));
    assert_eq!(usage.context_window_max_tokens, Some(32_000));
    session
        .start_turn("Continue", &fixture.spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    session.close().await.unwrap();
}
