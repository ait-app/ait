//! Native limits remain visible after history settlement and allow a later user continuation.
use super::*;

#[tokio::test]
async fn native_stop_reasons_settle_history_and_preserve_the_next_turn() {
    for reason in [
        "end_turn",
        "cancelled",
        "max_tokens",
        "max_turn_requests",
        "refusal",
    ] {
        let (root, client, spec) = fixture(&format!("stop-{reason}"));
        let mut session = client.create_session(&spec).await.unwrap();
        let turn = session.start_turn("work", &spec.config).await.unwrap();
        let mut history = Vec::new();
        loop {
            match event(session.as_mut()).await {
                AgentTurnEvent::History(entries) => history = entries,
                AgentTurnEvent::Completed(_) => {
                    assert_eq!(reason, "end_turn");
                    break;
                }
                AgentTurnEvent::Cancelled => {
                    assert_eq!(reason, "cancelled");
                    break;
                }
                AgentTurnEvent::Failed => {
                    assert!(matches!(
                        reason,
                        "max_tokens" | "max_turn_requests" | "refusal"
                    ));
                    break;
                }
                _ => {}
            }
        }
        let expected = match reason {
            "max_tokens" => {
                Some("OpenCode reached the model output limit. Send Continue to resume.")
            }
            "max_turn_requests" => {
                Some("OpenCode reached its turn request limit. Send Continue to resume.")
            }
            "refusal" => Some("OpenCode refused this request. Revise it before continuing."),
            _ => None,
        };
        assert_eq!(session.failure_message().as_deref(), expected);
        assert_eq!(history.len(), 2);
        assert!(
            history
                .iter()
                .all(|entry| entry.turn_id.as_deref() == Some(&turn))
        );
        let handle = session.persistence().unwrap();
        assert_eq!(session::clients(&handle).unwrap()["user1"], turn);

        session.start_turn("Continue", &spec.config).await.unwrap();
        assert!(session.failure_message().is_none());
        assert_eq!(completed(session.as_mut()).await.len(), 4);
        session.close().await.unwrap();
        assert_eq!(
            requests(&root)
                .iter()
                .filter(|request| request["method"] == "session/prompt")
                .count(),
            2
        );
    }
}

#[tokio::test]
async fn zero_native_usage_does_not_claim_the_conversation_is_empty() {
    let (_root, client, spec) = fixture("zero-usage");
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("work", &spec.config).await.unwrap();
    let mut usage = None;
    loop {
        match event(session.as_mut()).await {
            AgentTurnEvent::Usage(snapshot) => usage = Some(snapshot),
            AgentTurnEvent::Completed(_) => break,
            AgentTurnEvent::Failed | AgentTurnEvent::Cancelled => panic!("unexpected stop"),
            _ => {}
        }
    }
    let usage = usage.unwrap();
    assert_eq!(usage.context_window_used_tokens, None);
    assert_eq!(usage.context_window_max_tokens, Some(32_000));
    assert_eq!(usage.total_cost_usd, Some(0.02));
    session.close().await.unwrap();
}

#[tokio::test]
async fn factual_acp_usage_is_preserved_when_a_later_prompt_reports_only_a_placeholder() {
    let (_root, client, spec) = fixture("zero-after-usage");
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("seed", &spec.config).await.unwrap();
    let mut usage = None;
    for prompt in ["seed", "continue"] {
        if prompt == "continue" {
            session.start_turn(prompt, &spec.config).await.unwrap();
        }
        loop {
            match event(session.as_mut()).await {
                AgentTurnEvent::Usage(snapshot) => usage = Some(snapshot),
                AgentTurnEvent::Completed(_) => break,
                AgentTurnEvent::Cancelled | AgentTurnEvent::Failed => panic!("unexpected stop"),
                _ => {}
            }
        }
        assert_eq!(
            usage.as_ref().unwrap().context_window_used_tokens,
            Some(100)
        );
    }
    session.close().await.unwrap();
}

#[tokio::test]
async fn an_unknown_native_stop_reason_invalidates_the_connection_without_resubmitting() {
    let (root, client, spec) = fixture("stop-future");
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("work", &spec.config).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match session.poll_turn() {
                Err(error) => {
                    assert_eq!(error, AgentSessionError::Failed);
                    break;
                }
                Ok(Some(AgentTurnEvent::Completed(_))) => panic!("unknown stop reported success"),
                Ok(Some(_)) => {}
                Ok(None) => tokio::time::sleep(Duration::from_millis(5)).await,
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(
        session
            .start_turn("Continue", &spec.config)
            .await
            .unwrap_err(),
        AgentSessionError::Rejected
    );
    assert_eq!(
        requests(&root)
            .iter()
            .filter(|request| request["method"] == "session/prompt")
            .count(),
        1
    );
}

#[tokio::test]
async fn native_limits_persist_a_visible_error_and_only_continue_on_the_next_user_input() {
    use crate::service::agent_manager::{AgentManager, AgentRegistration};
    use domain::agent_runtime::{AgentRuntimeStatus, registry::AgentRuntimeRegistry};
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

    for reason in ["max_tokens", "max_turn_requests", "refusal"] {
        let (root, client, spec) = fixture(&format!("stop-{reason}"));
        let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
        registry.initialize().unwrap();
        let timeline = crate::storage::timeline::Timeline::memory().unwrap();
        let mut manager =
            AgentManager::new(Box::new(registry.clone())).with_timeline(timeline.clone());
        manager.register_client(Box::new(client)).unwrap();
        manager
            .create("limit-agent", &spec, AgentRegistration::default())
            .await
            .unwrap();
        manager.send("limit-agent", "work").await.unwrap();
        poll_until_idle(&mut manager, "limit-agent").await;
        let record = registry.get("limit-agent").unwrap().unwrap();
        assert_eq!(record.last_status, AgentRuntimeStatus::Error);
        let message = record.last_error.unwrap();
        let (_, rows) = timeline.read("limit-agent").unwrap();
        assert!(rows.iter().any(|row| row.entry.item["message"] == message));
        assert_eq!(
            requests(&root)
                .iter()
                .filter(|request| request["method"] == "session/prompt")
                .count(),
            1
        );

        manager.send("limit-agent", "Continue").await.unwrap();
        poll_until_idle(&mut manager, "limit-agent").await;
        let recovered = registry.get("limit-agent").unwrap().unwrap();
        assert_eq!(recovered.last_status, AgentRuntimeStatus::Idle);
        assert!(recovered.last_error.is_none());
        manager.close_all().await.unwrap();
    }
}
