//! Usage reads, built-in slash commands and runtime events (B8: D25, D26, M5, M9).
use super::*;

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

fn note(text: &str) -> impl Fn(&AgentTurnEvent) -> bool + '_ {
    move |event| matches!(event, AgentTurnEvent::Timeline(entry) if entry.item["text"] == text)
}

fn compactions(events: &[AgentTurnEvent]) -> Vec<&Value> {
    timeline_items(events)
        .into_iter()
        .filter(|item| item["type"] == "compaction")
        .collect()
}

fn usage(
    events: &[AgentTurnEvent],
) -> Option<(usize, &provider_core::protocol::usage::AgentUsage)> {
    events
        .iter()
        .enumerate()
        .find_map(|(index, event)| match event {
            AgentTurnEvent::Usage(usage) => Some((index, usage)),
            _ => None,
        })
}

fn command(text: &str) -> AgentPrompt {
    AgentPrompt::text(text)
}

#[tokio::test]
async fn session_stats_are_published_before_the_turn_outcome() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("hello", &spec.config).await.unwrap();

    let events = complete(session.as_mut()).await;

    // Two answered messages of 10 input, 2 cached and 5 output tokens at $0.001 each.
    let (position, usage) = usage(&events).expect("no usage before the outcome");
    assert!(position < events.len() - 1);
    assert_eq!(usage.input_tokens, Some(20));
    assert_eq!(usage.cached_input_tokens, Some(4));
    assert_eq!(usage.output_tokens, Some(10));
    assert_eq!(usage.total_cost_usd, Some(0.002));
    assert_eq!(usage.context_window_max_tokens, Some(32768));
    assert_eq!(usage.context_window_used_tokens, Some(17));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, AgentTurnEvent::Usage(_)))
            .count(),
        1
    );
    session.close().await.unwrap();
    assert!(count(&calls_after(root.path(), 0), "get_session_stats") >= 1);
}

#[tokio::test]
async fn without_session_stats_usage_falls_back_to_state_context_and_message_totals() {
    let (root, client, spec) = fixture_with(&[("OMP_FIXTURE_REJECT_STATS", "1")]);
    let mut session = client.create_session(&spec).await.unwrap();
    for _ in 0..2 {
        session.start_turn("hello", &spec.config).await.unwrap();
        let events = complete(session.as_mut()).await;
        let (_, usage) = usage(&events).expect("no fallback usage");
        assert_eq!(usage.context_window_max_tokens, Some(32768));
        assert_eq!(usage.context_window_used_tokens, Some(17));
        assert!(usage.input_tokens.is_some_and(|tokens| tokens >= 20));
    }
    session.close().await.unwrap();
    // A refusal is remembered; later reads ask get_state directly.
    assert_eq!(count(&calls_after(root.path(), 0), "get_session_stats"), 1);
}

#[tokio::test]
async fn compact_runs_beside_other_commands_with_its_own_rows_and_refreshes_usage() {
    let (root, client, spec) = fixture_with(&[("OMP_FIXTURE_HOLD_COMPACT", "1")]);
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("hello", &spec.config).await.unwrap();
    complete(session.as_mut()).await;

    session
        .out_of_band(&command("/compact keep it short"))
        .await
        .unwrap();
    assert!(matches!(
        session.out_of_band(&command("/compact")).await,
        Err(AgentSessionError::Rejected)
    ));
    // OMP is still compacting; another command is answered meanwhile.
    session
        .out_of_band(&command("/autocompact off"))
        .await
        .unwrap();
    let before = until(session.as_mut(), note("Auto-compaction disabled.")).await;
    std::fs::File::create(root.path().join("agent/release-compact")).unwrap();
    let after = until(session.as_mut(), |event| {
        matches!(event, AgentTurnEvent::Usage(_))
    })
    .await;

    let rows = compactions(&before);
    assert_eq!(rows.len(), 1, "{before:?}");
    assert_eq!(rows[0]["status"], "loading");
    assert_eq!(rows[0]["trigger"], "manual");
    let rows: Vec<_> = rows.into_iter().chain(compactions(&after)).collect();
    // OMP's manual_compaction_start/_end are hidden while Ait owns the rows.
    assert_eq!(rows.len(), 2, "{after:?}");
    assert_eq!(rows[1]["status"], "completed");
    assert_eq!(rows[1]["preTokens"], 17);
    assert_eq!(rows[0]["compactionId"], rows[1]["compactionId"]);
    // The compacted session reports only its context.
    let (_, usage) = usage(&after).unwrap();
    assert_eq!(usage.input_tokens, Some(0));
    assert_eq!(usage.context_window_used_tokens, Some(8586));
    session.close().await.unwrap();
    let calls = calls_after(root.path(), 0);
    let compact: Vec<_> = calls
        .iter()
        .filter(|call| call["type"] == "compact")
        .collect();
    assert_eq!(compact.len(), 1);
    assert_eq!(compact[0]["customInstructions"], "keep it short");
}

#[tokio::test]
async fn autocompact_sets_or_toggles_omps_setting() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();

    session.out_of_band(&command("/autocompact")).await.unwrap();
    until(session.as_mut(), note("Auto-compaction disabled.")).await;
    session
        .out_of_band(&command("/AUTOCOMPACT toggle"))
        .await
        .unwrap();
    until(session.as_mut(), note("Auto-compaction enabled.")).await;
    session
        .out_of_band(&command("/autocompact sometimes"))
        .await
        .unwrap();
    until(
        session.as_mut(),
        note("[Error] Usage: /autocompact [on|off|toggle]"),
    )
    .await;
    session.close().await.unwrap();

    let calls = calls_after(root.path(), 0);
    let settings: Vec<_> = calls
        .iter()
        .filter(|call| call["type"] == "set_auto_compaction")
        .map(|call| call["enabled"].clone())
        .collect();
    assert_eq!(settings, [json!(false), json!(true)]);
}

#[tokio::test]
async fn compaction_and_handoff_refusals_are_shown_as_paseo_shows_them() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let image = AgentPrompt {
        images: vec![PromptImage {
            data: "aW1hZ2U=".to_owned(),
            mime_type: "image/png".to_owned(),
        }],
        ..command("/compact")
    };
    assert!(matches!(
        session.out_of_band(&image).await,
        Err(AgentSessionError::Rejected)
    ));
    assert!(matches!(
        session.out_of_band(&command("/review")).await,
        Err(AgentSessionError::Rejected)
    ));

    session.out_of_band(&command("/compact")).await.unwrap();
    let events = until(
        session.as_mut(),
        note("[Error] Failed to compact context: Nothing to compact (session too small)"),
    )
    .await;
    let rows = compactions(&events);
    assert_eq!(rows.len(), 2, "{events:?}");
    assert_eq!(rows[1]["status"], "completed");
    assert!(rows[1].get("preTokens").is_none());
    session.out_of_band(&command("/handoff")).await.unwrap();
    until(
        session.as_mut(),
        note("[Error] Failed to hand off turn: Nothing to hand off (already compacted)"),
    )
    .await;

    session.start_turn("hello", &spec.config).await.unwrap();
    complete(session.as_mut()).await;
    let reads = count(&calls_after(root.path(), 0), "get_session_stats");
    session
        .out_of_band(&command("/handoff next"))
        .await
        .unwrap();
    // A handoff replaces the context, so usage is read again.
    let events = until(session.as_mut(), |event| {
        matches!(event, AgentTurnEvent::Usage(_))
    })
    .await;
    assert!(
        !timeline_items(&events).iter().any(|item| item["text"]
            .as_str()
            .is_some_and(|text| text.starts_with("[Error]"))),
        "{events:?}"
    );
    session.close().await.unwrap();
    let calls = calls_after(root.path(), 0);
    assert!(count(&calls, "get_session_stats") > reads);
    let handoff = calls.iter().find(|call| call["type"] == "handoff").unwrap();
    assert!(handoff.get("customInstructions").is_none());
    assert!(
        calls
            .iter()
            .any(|call| call["type"] == "handoff" && call["customInstructions"] == "next")
    );
}

#[tokio::test]
async fn steer_and_follow_up_start_an_announced_turn_when_idle_and_queue_when_busy() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();

    session
        .out_of_band(&input("/steer go left", "c1"))
        .await
        .unwrap();
    assert!(session.pending_foreground());
    let AgentTurnEvent::Started(_) = next(session.as_mut()).await.unwrap() else {
        panic!("an idle /steer did not announce its turn");
    };
    let events = complete(session.as_mut()).await;
    let users = user_messages(&events);
    assert_eq!(users[0].item["text"], "go left");
    assert_eq!(users[0].item["clientMessageId"], "c1");
    assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));

    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    until(
        session.as_mut(),
        |event| matches!(event, AgentTurnEvent::Timeline(entry) if entry.item["text"] == "wait"),
    )
    .await;
    session
        .out_of_band(&input("/follow-up then this", "c2"))
        .await
        .unwrap();
    assert!(!session.pending_foreground());
    let events = complete(session.as_mut()).await;
    let users = user_messages(&events);
    assert_eq!(users[0].item["text"], "then this");
    assert_eq!(users[0].item["clientMessageId"], "c2");
    assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentTurnEvent::Started(started) if *started != turn))
    );
    session.close().await.unwrap();

    let calls = calls_after(root.path(), 0);
    let prompts: Vec<_> = calls
        .iter()
        .filter(|call| call["type"] == "prompt")
        .map(|call| (call["message"].clone(), call["streamingBehavior"].clone()))
        .collect();
    assert_eq!(
        prompts,
        [
            (json!("go left"), json!("steer")),
            (json!("wait"), Value::Null),
            (json!("then this"), json!("followUp")),
        ]
    );
}

#[tokio::test]
async fn a_refused_queued_steer_warns_and_keeps_the_turn() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("steer-reject", &spec.config)
        .await
        .unwrap();
    session
        .out_of_band(&input("/steer go right", "c1"))
        .await
        .unwrap();

    let events = complete(session.as_mut()).await;

    assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
    let warning = timeline_items(&events)
        .into_iter()
        .find(|item| item["type"] == "notification")
        .expect("no refusal warning");
    assert_eq!(warning["level"], "warning");
    assert!(!warning["message"].to_string().contains("fixture-secret"));
    session.close().await.unwrap();
}

#[tokio::test]
async fn model_changes_publish_omps_runtime_facts() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let config = StoredAgentConfig {
        model: Some(json!(["other", "model"]).to_string()),
        ..spec.config.clone()
    };
    session.start_turn("hello", &config).await.unwrap();

    let events = until(session.as_mut(), |event| {
        matches!(event, AgentTurnEvent::RuntimeInfo(_))
    })
    .await;

    let Some(AgentTurnEvent::RuntimeInfo(info)) = events.last() else {
        unreachable!();
    };
    assert_eq!(info.model, config.model);
    assert_eq!(info.mode_id.as_deref(), Some("full"));
    session.close().await.unwrap();
}
