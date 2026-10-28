//! Epoch isolation, contradictory state and bounded user cancellation.
use super::*;
use crate::local::omp::tests::{complete, fixture, next};

#[tokio::test]
async fn steering_acknowledgements_are_correlated_and_failures_are_visible() {
    let (_root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    session
        .input_requests
        .insert("previous-input".to_owned(), "previous-turn".to_owned());
    session
        .consume(&json!({"type":"response","id":"previous-input",
        "command":"steer","success":false,"error":"Old rejection"}))
        .unwrap();
    assert!(session.failure_message().is_none());
    session
        .input_requests
        .insert("current-input".to_owned(), turn);

    let result = session.consume(&json!({"type":"response","id":"current-input",
        "command":"steer","success":false,"error":"Native input handler rejected this answer"}));

    assert_eq!(result, Err(AgentSessionError::Failed));
    assert!(
        session
            .failure_message()
            .unwrap()
            .contains("Native input handler rejected")
    );
    assert_eq!(session.input_requests, BTreeMap::new());
    session.close().await.unwrap();
}

#[tokio::test]
async fn unacknowledged_steering_is_bounded_before_more_input_is_submitted() {
    let (_root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    for index in 0..128 {
        session
            .input_requests
            .insert(format!("input-{index}"), turn.clone());
    }
    let pending = session.stream.pending_inputs.clone();

    assert_eq!(
        session.steer_turn(&turn, "extra input").await,
        Err(AgentSessionError::Rejected)
    );
    assert_eq!(session.stream.pending_inputs, pending);
    session.close().await.unwrap();
}

#[tokio::test]
async fn newly_accepted_or_started_work_invalidates_an_earlier_idle_check() {
    for work in ["steer", "agent_start"] {
        let (_root, client, spec) = fixture();
        let mut session = open(&client, &spec, None).await.unwrap();
        let turn = session.start_turn("wait", &spec.config).await.unwrap();
        session.phase = TurnPhase::AwaitingSettlement;
        session.state_check = Some(StateCheck {
            id: "old-state".to_owned(),
            turn: Some(turn.clone()),
            written: None,
        });
        session.idle_without_outcome = Some(Instant::now() - STATE_CHECK_INTERVAL);

        if work == "steer" {
            session.steer_turn(&turn, "continue").await.unwrap();
        } else {
            session.consume(&json!({"type":"agent_start"})).unwrap();
        }
        session
            .consume(
                &json!({"type":"response","command":"get_state","id":"old-state",
            "success":true,"data":{"sessionId":session.state["sessionId"],"isSettled":true}}),
            )
            .unwrap();

        assert_eq!(session.active.as_deref(), Some(turn.as_str()), "{work}");
        assert!(session.idle_without_outcome.is_none(), "{work}");
        assert!(session.stream.events.is_empty(), "{work}");
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn reconfiguration_discards_checks_owned_by_the_previous_native_process() {
    let (_root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    let (sender, receiver) = oneshot::channel();
    drop(sender);
    session.state_check = Some(StateCheck {
        id: "old-state".to_owned(),
        turn: Some(turn.clone()),
        written: Some(receiver),
    });
    session.cancel_requests.insert("old-abort".to_owned(), turn);
    session.finish();
    session.stream.events.clear();
    let config = StoredAgentConfig {
        mode_id: Some("ask".to_owned()),
        ..spec.config.clone()
    };

    session.configure(&config).await.unwrap();

    assert!(session.state_check.is_none());
    assert!(session.cancel_requests.is_empty());
    session.start_turn("hello", &config).await.unwrap();
    assert!(matches!(
        complete(&mut session).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn an_unresponsive_abort_expires_and_keeps_a_visible_reason_after_close() {
    let (_root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    let turn = session
        .start_turn("wait-stuck-abort", &spec.config)
        .await
        .unwrap();
    next(&mut session).await.unwrap();
    session.cancel_turn(&turn).await.unwrap();
    session.cancellation_started = Some(Instant::now() - CANCEL_GRACE);

    assert_eq!(session.poll_turn().unwrap_err(), AgentSessionError::Failed);
    assert_eq!(
        session.failure_diagnostic().unwrap().category,
        Category::Timeout
    );
    let reason = session.failure_message().unwrap().into_owned();
    assert!(reason.contains("did not confirm cancellation"));
    session.close().await.unwrap();
    assert_eq!(session.failure_message().as_deref(), Some(reason.as_str()));
}

#[tokio::test]
async fn a_previous_state_response_cannot_finish_the_next_admitted_turn() {
    let (_root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    let previous = session.start_turn("wait", &spec.config).await.unwrap();
    session.state_check = Some(StateCheck {
        id: "old-state".to_owned(),
        turn: Some(previous),
        written: None,
    });
    session.finish();
    session.stream.events.clear();
    let current = session.start_turn("wait", &spec.config).await.unwrap();

    session
        .consume(
            &json!({"type":"response","command":"get_state","id":"old-state",
        "success":true,"data":{"sessionId":session.state["sessionId"],"isSettled":true}}),
        )
        .unwrap();

    assert_eq!(session.active.as_deref(), Some(current.as_str()));
    assert!(session.stream.events.is_empty());
    session.close().await.unwrap();
}

#[tokio::test]
async fn busy_or_missing_state_facts_never_complete_the_active_turn() {
    let (_root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    session.phase = TurnPhase::AwaitingSettlement;
    for busy in [
        json!({"isStreaming":true}),
        json!({"isCompacting":true}),
        json!({"hasPendingAsyncWork":true}),
        json!({"queuedMessageCount":1}),
        json!({"isSettled":false}),
        json!({"isSettled":null}),
    ] {
        session.state_check = Some(StateCheck {
            id: "state".to_owned(),
            turn: Some(turn.clone()),
            written: None,
        });
        let mut state = json!({"sessionId":session.state["sessionId"],"isSettled":true});
        state
            .as_object_mut()
            .unwrap()
            .extend(busy.as_object().unwrap().clone());
        session
            .consume(
                &json!({"type":"response","command":"get_state","id":"state",
            "success":true,"data":state}),
            )
            .unwrap();
        assert_eq!(session.active.as_deref(), Some(turn.as_str()));
        assert!(session.stream.events.is_empty());
    }
    session.close().await.unwrap();
}

#[tokio::test]
async fn another_native_sessions_settlement_is_rejected() {
    let (_root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    session.state_check = Some(StateCheck {
        id: "state".to_owned(),
        turn: Some(turn),
        written: None,
    });

    assert_eq!(
        session
            .consume(
                &json!({"type":"response","command":"get_state","id":"state",
        "success":true,"data":{"sessionId":"another-session","isSettled":true}})
            )
            .unwrap_err(),
        AgentSessionError::Failed
    );
    assert!(
        session
            .failure_message()
            .unwrap()
            .contains("different native session")
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn a_failed_state_write_invalidates_the_connection() {
    let (_root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    let (sender, receiver) = oneshot::channel();
    sender.send(Err(AgentSessionError::Failed)).unwrap();
    session.state_check = Some(StateCheck {
        id: "state".to_owned(),
        turn: Some(turn),
        written: Some(receiver),
    });

    assert_eq!(
        session.poll_state_check().unwrap_err(),
        AgentSessionError::Failed
    );
    assert!(
        session
            .failure_message()
            .unwrap()
            .contains("could not write an OMP state check")
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn a_state_response_can_precede_the_correlated_prompt_result() {
    let (_root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    session.state_check = Some(StateCheck {
        id: "state".to_owned(),
        turn: Some(turn.clone()),
        written: None,
    });
    session
        .consume(
            &json!({"type":"response","command":"get_state","id":"state",
        "success":true,"data":{"sessionId":session.state["sessionId"],"isSettled":true}}),
        )
        .unwrap();
    assert_eq!(session.active.as_deref(), Some(turn.as_str()));
    assert!(!session.stream.failed);

    session
        .consume(&json!({"type":"prompt_result","id":session.prompt_id,
        "status":"completed","sessionSettled":true}))
        .unwrap();
    assert!(session.active.is_none());
    assert!(matches!(
        session.stream.events.back(),
        Some(AgentTurnEvent::Completed(_))
    ));
    session.close().await.unwrap();
}
