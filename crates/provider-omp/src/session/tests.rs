//! Epoch isolation, contradictory state and bounded user cancellation.
use super::*;
use crate::tests::{calls_after, complete, confinement, count, fixture, launches, next};

fn with_fast(config: &StoredAgentConfig) -> StoredAgentConfig {
    StoredAgentConfig {
        feature_values: Some(BTreeMap::from([("fastMode".to_owned(), json!(true))])),
        ..config.clone()
    }
}

#[tokio::test]
async fn late_steering_rejection_cannot_claim_the_next_prompts_client_message_id() {
    let (_root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    session
        .stream
        .message(&json!({"role":"user","timestamp":1000,"content":"wait"}))
        .unwrap();
    session
        .steer_input(
            &turn,
            &AgentPrompt {
                client_message_id: Some("rejected-input".into()),
                ..AgentPrompt::text("continue")
            },
        )
        .await
        .unwrap();
    let request = session.input_requests.keys().next().unwrap().clone();
    session.finish();
    session.stream.events.clear();
    session
        .start_input(
            &AgentPrompt {
                client_message_id: Some("next-input".into()),
                ..AgentPrompt::text("wait")
            },
            &spec.config,
        )
        .await
        .unwrap();

    session
        .consume(&json!({"type":"response","id":request,"command":"steer",
            "success":false,"error":"Previous input rejected"}))
        .unwrap();
    session
        .stream
        .message(&json!({"role":"user","timestamp":2000,"content":"wait"}))
        .unwrap();

    assert!(
        matches!(session.stream.events.back(), Some(AgentTurnEvent::Timeline(entry))
        if entry.item["clientMessageId"] == "next-input")
    );
    assert!(session.stream.pending_inputs.is_empty());
    session.close().await.unwrap();
}

#[tokio::test]
async fn steering_acknowledgements_are_correlated_and_late_rejections_keep_the_turn() {
    let (_root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    session
        .input_requests
        .insert("previous-input".to_owned(), "previous-turn".to_owned());
    session.stream.pending_inputs.push_back((
        "previous-input".to_owned(),
        Some("previous-message".to_owned()),
    ));
    session
        .consume(&json!({"type":"response","id":"previous-input",
        "command":"steer","success":false,"error":"Old rejection"}))
        .unwrap();
    assert!(session.failure_message().is_none());
    assert!(
        session
            .stream
            .pending_inputs
            .iter()
            .all(|(id, _)| id != "previous-input")
    );
    session
        .input_requests
        .insert("current-input".to_owned(), turn.clone());
    session.stream.pending_inputs.push_back((
        "current-input".to_owned(),
        Some("current-message".to_owned()),
    ));
    session.stream.pending_inputs.push_back((
        "another-input".to_owned(),
        Some("another-message".to_owned()),
    ));

    session.stream.events.clear();

    session
        .consume(
            &json!({"type":"response","id":"current-input","command":"steer",
            "success":false,"error":"Native input handler rejected this answer api_key=secret-1"}),
        )
        .unwrap();

    // D6: a late rejection is shown and the turn keeps running.
    assert_eq!(session.active.as_deref(), Some(turn.as_str()));
    assert!(session.failure_message().is_none());
    let Some(AgentTurnEvent::Timeline(notice)) = session.stream.events.pop_front() else {
        panic!("expected a rejection notification");
    };
    assert_eq!(notice.item["type"], "notification");
    assert_eq!(notice.item["level"], "warning");
    let text = notice.item["message"].as_str().unwrap();
    assert!(text.starts_with(STEER_REJECTED), "{text}");
    assert!(text.contains("Native input handler rejected"), "{text}");
    assert!(!text.contains("secret-1"), "{text}");
    assert_eq!(session.input_requests, BTreeMap::new());
    assert!(
        session
            .stream
            .pending_inputs
            .iter()
            .all(|(id, _)| id != "current-input")
    );
    assert_eq!(
        session.stream.pending_inputs.back().unwrap().0,
        "another-input"
    );
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
            after_turn: false,
            model: false,
            deadline: Instant::now() + Duration::from_secs(60),
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
        after_turn: false,
        model: false,
        deadline: Instant::now() + Duration::from_secs(60),
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
    session.cancellation = Cancellation::Requested(Instant::now() - CANCEL_GRACE);

    assert_eq!(session.poll_turn(), Ok(Some(AgentTurnEvent::Failed)));
    assert!(session.active.is_none());
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
        after_turn: false,
        model: false,
        deadline: Instant::now() + Duration::from_secs(60),
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
            after_turn: false,
            model: false,
            deadline: Instant::now() + Duration::from_secs(60),
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

/// A state check `id`, due later: of `turn`'s settlement, or after a turn without one.
fn state_check(id: &str, turn: Option<String>) -> StateCheck {
    StateCheck {
        id: id.to_owned(),
        after_turn: turn.is_none(),
        turn,
        written: None,
        model: false,
        deadline: Instant::now() + Duration::from_secs(60),
    }
}

/// A `get_state` response `id` naming native session `session` with file `file`.
fn switched_state(session: &Session, id: &str, native: &str, file: &str) -> Value {
    let mut state = session.state.clone();
    state["sessionId"] = json!(native);
    state["sessionFile"] = json!(file);
    state["isSettled"] = json!(true);
    json!({"type":"response","command":"get_state","id":id,"success":true,"data":state})
}

#[tokio::test]
async fn a_native_session_switch_is_followed_after_and_during_a_turn() {
    let (root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    session.start_turn("hello", &spec.config).await.unwrap();
    complete(&mut session).await;
    let switched = Uuid::new_v4().to_string();
    let file = root.path().join("switched.jsonl");
    let file = file.to_str().unwrap();
    session.state_check = Some(state_check("after", None));
    session.stream.events.clear();

    // An extension command switched OMP's session; the post-turn read reports it.
    session
        .consume(&switched_state(&session, "after", &switched, file))
        .unwrap();

    let handle = session.persistence().unwrap();
    assert_eq!(handle.session_id, switched);
    assert_eq!(handle.metadata.unwrap()["sessionFile"], file);
    assert!(session.stream.events.iter().any(|event| matches!(event,
        AgentTurnEvent::RuntimeInfo(info) if info.session_id.as_deref() == Some(&switched))));

    // A settlement check during a turn follows a switch instead of closing OMP.
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    let again = Uuid::new_v4().to_string();
    session.state_check = Some(state_check("during", Some(turn.clone())));
    session
        .consume(&switched_state(&session, "during", &again, file))
        .unwrap();
    assert_eq!(session.state["sessionId"], again);
    assert_eq!(session.active.as_deref(), Some(turn.as_str()));

    // A session id that is no UUID is still a protocol failure.
    session.state_check = Some(state_check("invalid", Some(turn)));
    assert_eq!(
        session
            .consume(&switched_state(
                &session,
                "invalid",
                "another-session",
                file
            ))
            .unwrap_err(),
        AgentSessionError::Failed
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn an_unanswered_state_check_expires_and_is_asked_again() {
    let (_root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    session.phase = TurnPhase::AwaitingSettlement;
    session.state_check = Some(StateCheck {
        model: true,
        deadline: Instant::now(),
        ..state_check("lost", Some(turn))
    });
    session.next_state_check = Instant::now();

    session.poll_state_check().unwrap();

    let check = session.state_check.as_ref().unwrap();
    assert_ne!(check.id, "lost");
    assert!(check.turn.is_some());
    // The model read the lost check carried is asked again with it.
    assert!(check.model);
    session.close().await.unwrap();
}

#[tokio::test]
async fn a_steer_after_a_local_prompt_cannot_take_its_client_message_id() {
    let (_root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    let local = AgentPrompt {
        client_message_id: Some("local-input".into()),
        ..AgentPrompt::text("/session")
    };
    let turn = session.start_input(&local, &spec.config).await.unwrap();
    let prompt = session.prompt_id.clone().unwrap();
    session
        .consume(
            &json!({"type":"response","id":prompt,"command":"prompt","success":true,
            "data":{"agentInvoked":false}}),
        )
        .unwrap();

    // OMP 18.18.8 accepts a steer while idle after a builtin command and runs it at once.
    let steer = AgentPrompt {
        client_message_id: Some("steer-input".into()),
        ..AgentPrompt::text("steered")
    };
    session.steer_input(&turn, &steer).await.unwrap();
    let request = session.input_requests.keys().next().unwrap().clone();
    for event in [
        json!({"type":"response","id":request,"command":"steer","success":true}),
        json!({"type":"agent_start"}),
        json!({"type":"message_end","message":{"role":"user","timestamp":2000,"steering":true,
            "content":[{"type":"text","text":"steered"}]}}),
        json!({"type":"session_settled"}),
    ] {
        session.consume(&event).unwrap();
    }

    let users: Vec<_> = session
        .stream
        .events
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Timeline(entry) if entry.item["type"] == "user_message" => Some((
                entry.item["text"].clone(),
                entry.item["clientMessageId"].clone(),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        users,
        [
            (json!("steered"), json!("steer-input")),
            (json!("/session"), json!("local-input"))
        ]
    );
    assert!(matches!(
        session.stream.events.back(),
        Some(AgentTurnEvent::Completed(_))
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn a_completed_turn_closes_tools_whose_results_never_arrived() {
    let (_root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    session.start_turn("wait", &spec.config).await.unwrap();
    let prompt = session.prompt_id.clone().unwrap();
    session
        .consume(&json!({"type":"tool_execution_start","toolCallId":"orphan",
            "toolName":"bash","args":{"command":"sleep 1"}}))
        .unwrap();
    session.stream.events.clear();

    session
        .consume(
            &json!({"type":"prompt_result","id":prompt,"status":"completed",
            "agentInvoked":true,"sessionSettled":true}),
        )
        .unwrap();

    let events: Vec<_> = session.stream.events.drain(..).collect();
    assert!(matches!(&events[0], AgentTurnEvent::Timeline(entry)
        if entry.key.ends_with(":tool:orphan") && entry.item["status"] == "canceled"));
    assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
    session.close().await.unwrap();
}

#[tokio::test]
async fn an_oversized_question_is_cancelled_and_the_turn_continues() {
    let (_root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    session.stream.events.clear();

    session
        .consume(
            &json!({"type":"extension_ui_request","id":"large","method":"input",
            "title":"x".repeat(70 * 1024)}),
        )
        .unwrap();

    assert!(session.pending_permissions().is_empty());
    assert_eq!(session.active.as_deref(), Some(turn.as_str()));
    assert!(
        matches!(session.stream.events.front(), Some(AgentTurnEvent::Timeline(entry))
        if entry.item["level"] == "warning"
            && entry.item["message"].as_str().unwrap().contains("64 KiB"))
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
        after_turn: false,
        model: false,
        deadline: Instant::now() + Duration::from_secs(60),
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
        after_turn: false,
        model: false,
        deadline: Instant::now() + Duration::from_secs(60),
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

#[tokio::test]
async fn a_restart_reopens_the_session_file_with_the_running_model_thinking_and_fast() {
    let (root, client, mut spec) = fixture();
    spec.config.model = Some(json!(["test", "model"]).to_string());
    spec.config.thinking_option_id = Some("high".to_owned());
    let mut session = open(&client, &spec, None).await.unwrap();
    session.start_turn("hello", &spec.config).await.unwrap();
    complete(&mut session).await;
    let fast = with_fast(&spec.config);
    session.configure(&fast).await.unwrap();
    let id = session.state["sessionId"].clone();
    let file = session.state["sessionFile"].as_str().unwrap().to_owned();
    let skip = calls_after(root.path(), 0).len();
    let config = StoredAgentConfig {
        mode_id: Some("ask".to_owned()),
        ..fast
    };

    session.configure(&config).await.unwrap();

    let calls = calls_after(root.path(), skip);
    assert_eq!(
        launches(&calls),
        [[
            "--mode",
            "rpc-ui",
            "--approval-mode",
            "always-ask",
            "--model",
            "test/model",
            "--thinking",
            "high",
            "--session",
            file.as_str(),
        ]]
    );
    assert_eq!(session.state["sessionId"], id);
    assert!(config::fast_mode(&session.state).unwrap().enabled);
    // OMP restored Fast from its session file, so the new process needs no request.
    assert_eq!(count(&calls, "set_fast_mode"), 0);
    session.start_turn("hello", &config).await.unwrap();
    assert!(matches!(
        complete(&mut session).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn a_restart_before_any_history_restores_fast_and_carries_omps_selection() {
    let (root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    let fast = with_fast(&spec.config);
    session.configure(&fast).await.unwrap();
    let skip = calls_after(root.path(), 0).len();
    let config = StoredAgentConfig {
        system_prompt: Some("Be brief.".to_owned()),
        ..fast
    };

    session.configure(&config).await.unwrap();

    let calls = calls_after(root.path(), skip);
    assert_eq!(
        launches(&calls),
        [[
            "--mode",
            "rpc-ui",
            "--approval-mode",
            "yolo",
            "--model",
            "test/model",
            "--thinking",
            "off",
            "--append-system-prompt",
            "Be brief.",
        ]]
    );
    assert_eq!(count(&calls, "set_fast_mode"), 1);
    assert!(config::fast_mode(&session.state).unwrap().enabled);
    session.close().await.unwrap();
}

#[tokio::test]
async fn a_failed_restart_keeps_the_running_process_and_its_write_confinement() {
    let (root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    // A confinement whose worktree cannot be resolved fails closed before any launch.
    let unresolvable = confinement(&root.path().join("missing-worktree"));
    session.confinement.clone_from(&unresolvable);
    let skip = calls_after(root.path(), 0).len();
    let config = StoredAgentConfig {
        mode_id: Some("ask".to_owned()),
        ..spec.config.clone()
    };

    assert!(session.configure(&config).await.is_err());

    assert_eq!(session.confinement, unresolvable);
    assert_eq!(session.config, spec.config);
    assert!(launches(&calls_after(root.path(), skip)).is_empty());
    session.start_turn("hello", &spec.config).await.unwrap();
    assert!(matches!(
        complete(&mut session).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    session.close().await.unwrap();
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn write_confinement_survives_a_restart() {
    // Outside the sandbox's temporary roots, so only an unconfined process can write it.
    let build = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    let outside = tempfile::tempdir_in(build).unwrap();
    let probe = outside.path().join("written-outside");
    let (root, client, mut spec) =
        crate::tests::fixture_with(&[("OMP_FIXTURE_WRITE_PROBE", probe.to_str().unwrap())]);
    std::fs::create_dir_all(root.path().join(".git")).unwrap();
    spec.confinement = confinement(root.path());
    let mut session = open(&client, &spec, None).await.unwrap();
    assert_eq!(session.confinement, spec.confinement);
    let config = StoredAgentConfig {
        mode_id: Some("write".to_owned()),
        ..spec.config.clone()
    };

    session.configure(&config).await.unwrap();
    session.close().await.unwrap();

    let probes: Vec<_> = calls_after(root.path(), 0)
        .into_iter()
        .filter_map(|call| call.get("writeProbe").cloned())
        .collect();
    assert_eq!(probes, [json!("denied"), json!("denied")]);
    assert!(!probe.exists());
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn a_confined_session_writes_its_state_but_not_omp_configuration() {
    let (root, client, mut spec) = crate::tests::fixture_with(&[("OMP_FIXTURE_STATE_PROBE", "1")]);
    std::fs::create_dir_all(root.path().join(".git")).unwrap();
    let config = root.path().join("agent/config.yml");
    std::fs::create_dir_all(root.path().join("agent")).unwrap();
    std::fs::write(&config, "theme: dark\n").unwrap();
    // The agent directory sits inside the writable worktree; protection still wins.
    spec.confinement = confinement(root.path());

    let mut session = open(&client, &spec, None).await.unwrap();
    session.close().await.unwrap();

    let probe = calls_after(root.path(), 0)
        .into_iter()
        .find_map(|call| call.get("stateProbe").cloned())
        .unwrap();
    assert_eq!(
        probe,
        json!({"sessions/probe":"written","blobs/probe":"written","agent.db":"written",
            "config.yml":"denied","models.yml":"denied","extensions/probe.ts":"denied"})
    );
    assert_eq!(std::fs::read_to_string(&config).unwrap(), "theme: dark\n");
    assert!(!root.path().join("agent/models.yml").exists());
    assert!(!root.path().join("agent/extensions").exists());
}

#[tokio::test]
async fn cancelling_after_the_process_exited_counts_as_stopped_and_reports_the_exit() {
    let (root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    let turn = session.start_turn("crash", &spec.config).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !session.transport.exited() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();

    session.cancel_turn(&turn).await.unwrap();

    assert_eq!(session.cancellation, Cancellation::None);
    assert_eq!(count(&calls_after(root.path(), 0), "abort"), 0);
    assert_eq!(
        complete(&mut session).await.last(),
        Some(&AgentTurnEvent::Failed)
    );
    assert!(
        session
            .failure_message()
            .unwrap()
            .contains("exited unexpectedly with code 3")
    );
    session.close().await.unwrap();
}

#[test]
fn slash_commands_follow_paseos_parse() {
    for text in ["/compact", " /compact now ", "/skill:review x"] {
        assert!(is_slash_command(text), "{text}");
    }
    for text in [
        "/",
        "/ compact",
        "/path/to/file is broken",
        "please /compact",
        "",
    ] {
        assert!(!is_slash_command(text), "{text}");
    }
}

#[test]
fn failure_text_lists_only_present_details_and_quotes_bounded_partial_output() {
    let message = json!({"stopReason":"error","provider":"p","model":"m","responseId":"",
        "content":[{"type":"thinking","thinking":"plan"},{"type":"toolCall","id":"x"},
            {"type":"text","text":format!("{}\"", "é".repeat(600))}]});

    let text = failure_text("Boom", &message);

    let quoted: String = format!("plan\n\n{}", "é".repeat(600))
        .chars()
        .take(FAILURE_PARTIAL_CHARS)
        .collect();
    assert_eq!(
        text,
        format!(
            "Boom (stopReason=error, model=p/m, partial={})",
            json!(quoted)
        )
    );
    assert_eq!(failure_text("Boom", &json!({"content":[]})), "Boom");
}

#[tokio::test]
async fn an_acknowledged_local_prompt_awaits_settlement_and_checks_native_state_at_once() {
    let (_root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    session.start_turn("wait", &spec.config).await.unwrap();
    let prompt = session.prompt_id.clone().unwrap();

    session
        .consume(
            &json!({"type":"response","id":prompt,"command":"prompt","success":true,
            "data":{"agentInvoked":false}}),
        )
        .unwrap();

    assert!(session.active.is_some());
    assert_eq!(session.phase, TurnPhase::AwaitingSettlement);
    assert!(session.next_state_check <= Instant::now());
    session.close().await.unwrap();
}
