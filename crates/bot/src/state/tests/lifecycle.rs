use super::*;
use domain::bot::DeliveryStatus;
use provider::service::bot_execution::Action;

fn admit(state: &mut super::super::State, workspace: &str, message: &str) -> domain::bot::Turn {
    let opened = state
        .execute(
            "bot.conversation.open.request",
            json!({"workspaceId":workspace}),
        )
        .unwrap();
    let accepted = state.execute("bot.message.send.request", json!({"workspaceId":workspace,"conversationId":opened["conversation"]["id"],"messageId":message,"text":"hello"})).unwrap();
    serde_json::from_value(accepted["turn"].clone()).unwrap()
}

#[test]
fn queue_budget_and_reset_conflicts_leave_the_active_turn_untouched() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    let first = admit(&mut state, WORKSPACE, "first");
    let run = state.ready().unwrap().remove(0);
    assert_eq!(
        state.execute(
            "bot.conversation.reset.request",
            json!({"workspaceId":WORKSPACE,"conversationId":first.conversation_id})
        ),
        Err(ErrorCode::BotConflict)
    );
    assert!(!run.cancel.is_cancelled());
    for index in 0..32 {
        admit(&mut state, WORKSPACE, &format!("queued-{index}"));
    }
    assert_eq!(state.execute("bot.message.send.request", json!({"workspaceId":WORKSPACE,"conversationId":first.conversation_id,"messageId":"overflow","text":"hello"})), Err(ErrorCode::ResourceExhausted));
    assert_eq!(
        fixture.store.turns(first.conversation_id).unwrap().len(),
        33
    );
    assert_eq!(admit(&mut state, WORKSPACE, "first").id, first.id);
    assert!(state.ready().unwrap().is_empty());
}

#[test]
fn channel_admission_retries_a_full_bot_backlog_but_refuses_a_full_conversation() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let channel = ChannelId::generate();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    state.execute("bot.channel.create.request", json!({"workspaceId":WORKSPACE,"channelId":channel,"enabled":true,"config":{"kind":"telegram","tokenRef":"env:AIT_SERVER_CREDENTIAL_TEST","allowedSenders":["1"]}})).unwrap();
    let message = |sender: usize, index: usize| {
        external(
            channel,
            &format!("s{sender}"),
            &format!("m{sender}-{index}"),
        )
    };
    for index in 0..32 {
        state.accept_channel(message(0, index)).unwrap();
    }

    assert_eq!(
        state.accept_channel(message(0, 32)),
        Err(ErrorCode::ResourceExhausted)
    );
    for sender in 1..16 {
        for index in 0..32 {
            state.accept_channel(message(sender, index)).unwrap();
        }
    }
    assert!(state.backlog_full());
    assert_eq!(
        state.accept_channel(message(16, 0)).unwrap(),
        json!({"accepted":false})
    );
    assert_eq!(
        state.accept(message(16, 0)),
        Err(ErrorCode::ResourceExhausted)
    );
}

#[test]
fn reset_of_an_uncertain_conversation_cancels_its_blocked_queue() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    let interrupted = admit(&mut state, WORKSPACE, "interrupted");
    assert_eq!(state.ready().unwrap().len(), 1);
    let queued = admit(&mut state, WORKSPACE, "queued");
    let mut restarted = fixture.state();
    assert_eq!(
        restarted.turn(interrupted.id).unwrap().status,
        TurnStatus::Uncertain
    );
    assert!(restarted.ready().unwrap().is_empty());

    restarted
        .execute(
            "bot.conversation.reset.request",
            json!({"workspaceId":WORKSPACE,"conversationId":queued.conversation_id}),
        )
        .unwrap();

    assert_eq!(
        restarted.turn(queued.id).unwrap().status,
        TurnStatus::Cancelled
    );
    let next = admit(&mut restarted, WORKSPACE, "after-reset");
    let ready = restarted.ready().unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].turn.id, next.id);
    assert_eq!(ready[0].turn.generation, 1);
}

#[test]
fn workspace_retirement_closes_sessions_and_cancels_queued_turns() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    admit(&mut state, WORKSPACE, "first");
    let running = state.ready().unwrap().remove(0);
    let queued = admit(&mut state, WORKSPACE, "second");
    let session = running.conversation.session_id.clone();

    state.retire(&[WORKSPACE.into()]).unwrap();

    assert!(running.cancel.is_cancelled());
    assert_eq!(state.turn(queued.id).unwrap().status, TurnStatus::Cancelled);
    assert!(
        matches!(&state.session_actions[0].1, Action::Close { agent_id } if agent_id == &session)
    );
    assert_eq!(
        state
            .execute("bot.get.request", json!({"workspaceId":WORKSPACE}))
            .unwrap(),
        json!({"bot":null})
    );
    assert!(matches!(
        fixture.store.catalog().unwrap().bots[0].state,
        domain::bot::BotState::Retired { .. }
    ));
    assert!(state.channels().is_empty());
}

#[test]
fn unreadable_workspace_memory_fails_only_that_workspace_turns() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    for workspace in [WORKSPACE, OTHER_WORKSPACE] {
        state
            .execute("bot.setup.request", setup(workspace))
            .unwrap();
    }
    let failed = admit(&mut state, WORKSPACE, "first");
    let healthy = admit(&mut state, OTHER_WORKSPACE, "first");
    let path = fixture.root.path().join("bots/memory").join(format!(
        "{:x}.md",
        <sha2::Sha256 as sha2::Digest>::digest(WORKSPACE.as_bytes())
    ));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, vec![b'a'; 32769]).unwrap();

    let ready = state.ready().unwrap();

    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].turn.id, healthy.id);
    assert_eq!(state.turn(failed.id).unwrap().status, TurnStatus::Failed);
    assert!(!state.frozen);
}

#[test]
fn recovered_outbox_resumes_only_replies_that_never_started_sending() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    let channel = ChannelId::generate();
    state.execute("bot.channel.create.request", json!({"workspaceId":WORKSPACE,"channelId":channel,"enabled":true,"config":{"kind":"telegram","tokenRef":"env:AIT_SERVER_CREDENTIAL_TELEGRAM","allowedSenders":["alice"]}})).unwrap();
    let accepted = state.accept(external(channel, "alice", "first")).unwrap();
    let first: BotTurnId = serde_json::from_value(accepted["turn"]["id"].clone()).unwrap();
    state
        .progress(first, TurnStatus::Completed, Some("reply".into()), None)
        .unwrap();
    let mut restarted = fixture.state();
    assert_eq!(
        restarted.pending_delivery(channel).unwrap().unwrap().0.id,
        first
    );
    restarted.delivery(first, DeliveryStatus::Sending).unwrap();
    assert!(restarted.pending_delivery(channel).unwrap().is_none());
    let recovered = fixture.state();
    assert_eq!(
        recovered.turn(first).unwrap().delivery,
        DeliveryStatus::Uncertain
    );
    assert!(recovered.pending_delivery(channel).unwrap().is_none());
}

#[test]
fn receipt_write_failure_freezes_private_admission_and_cursor_updates() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let channel = ChannelId::generate();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    state.execute("bot.channel.create.request", json!({"workspaceId":WORKSPACE,"channelId":channel,"enabled":true,"config":{"kind":"telegram","tokenRef":"env:AIT_SERVER_CREDENTIAL_TELEGRAM","allowedSenders":["alice"]}})).unwrap();
    let path = fixture.root.path().join("bots/turns");
    std::fs::write(&path, "unavailable directory").unwrap();
    assert_eq!(
        state.accept(external(channel, "alice", "first")),
        Err(ErrorCode::BotIo)
    );
    assert!(state.frozen);
    std::fs::remove_file(path).unwrap();
    assert_eq!(
        state.accept(external(channel, "alice", "first")),
        Err(ErrorCode::BotIo)
    );
    assert_eq!(state.advance_cursor(channel, 1), Err(ErrorCode::BotIo));
    assert!(state.ready().unwrap().is_empty());
    let stored = fixture.store.catalog().unwrap();
    assert!(
        stored
            .channels
            .iter()
            .find(|entry| entry.id == channel)
            .unwrap()
            .cursor
            .is_none()
    );
    assert!(
        fixture
            .store
            .turns(fixture.store.conversations().unwrap()[0].id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn cancel_stops_a_running_turn_and_completed_turns_stay_final() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    let turn = admit(&mut state, WORKSPACE, "permission");
    let run = state.ready().unwrap().remove(0);
    state
        .progress(turn.id, TurnStatus::NeedsApproval, None, None)
        .unwrap();

    let cancelled = state
        .cancel(&serde_json::from_value(json!({"workspaceId":WORKSPACE,"turnId":turn.id})).unwrap())
        .unwrap();
    assert_eq!(cancelled["cancellationRequested"], true);
    assert!(run.cancel.is_cancelled());
    state.finished(turn.conversation_id);
    state
        .progress(
            turn.id,
            TurnStatus::Completed,
            Some("completed reply".into()),
            None,
        )
        .unwrap();
    let unchanged = state
        .cancel(&serde_json::from_value(json!({"workspaceId":WORKSPACE,"turnId":turn.id})).unwrap())
        .unwrap();
    assert_eq!(unchanged["turn"]["status"], "completed");
    state
        .progress(
            turn.id,
            TurnStatus::Failed,
            None,
            Some("late failure".into()),
        )
        .unwrap();
    assert_eq!(
        state.turn(turn.id).unwrap().reply.as_deref(),
        Some("completed reply")
    );
}

#[test]
fn catalog_and_conversation_write_failures_freeze_admission_until_restart() {
    for catalog_failure in [false, true] {
        let fixture = Fixture::new();
        let mut state = fixture.state();
        state
            .execute("bot.setup.request", setup(WORKSPACE))
            .unwrap();
        let result = if catalog_failure {
            let path = fixture.root.path().join("bots/catalog.json");
            std::fs::remove_file(&path).unwrap();
            std::fs::create_dir(&path).unwrap();
            state.execute(
                "bot.update.request",
                json!({"workspaceId":WORKSPACE,"execution":{"provider":"other"}}),
            )
        } else {
            std::fs::write(
                fixture.root.path().join("bots/conversations"),
                "fixture blocker",
            )
            .unwrap();
            state.execute(
                "bot.conversation.open.request",
                json!({"workspaceId":WORKSPACE}),
            )
        };

        assert_eq!(result, Err(ErrorCode::BotIo));
        assert!(state.frozen);
        assert_eq!(
            state.execute("bot.get.request", json!({"workspaceId":WORKSPACE})),
            Err(ErrorCode::BotIo)
        );
        assert!(state.ready().unwrap().is_empty());
    }
}

fn bot_state(state: &mut super::super::State) -> serde_json::Value {
    state
        .execute("bot.get.request", json!({"workspaceId":WORKSPACE}))
        .unwrap()["bot"]["state"]
        .clone()
}

fn remember(state: &mut super::super::State, text: &str) {
    let revision = state
        .execute("bot.memory.get.request", json!({"workspaceId":WORKSPACE}))
        .unwrap()["revision"]
        .clone();
    state
        .execute(
            "bot.memory.update.request",
            json!({"workspaceId":WORKSPACE,"text":text,"expected":revision}),
        )
        .unwrap();
}

fn recalled(state: &mut super::super::State) -> serde_json::Value {
    state
        .execute("bot.memory.get.request", json!({"workspaceId":WORKSPACE}))
        .unwrap()["text"]
        .clone()
}

fn create_telegram(
    state: &mut super::super::State,
    channel: ChannelId,
) -> Result<serde_json::Value, ErrorCode> {
    state.execute(
        "bot.channel.create.request",
        json!({"workspaceId":WORKSPACE,"channelId":channel,"enabled":true,"config":{"kind":"telegram","tokenRef":"env:AIT_SERVER_CREDENTIAL_TELEGRAM","allowedSenders":["alice"]}}),
    )
}

fn send(
    state: &mut super::super::State,
    conversation: ConversationId,
    message: &str,
) -> Result<serde_json::Value, ErrorCode> {
    state.execute(
        "bot.message.send.request",
        json!({"workspaceId":WORKSPACE,"conversationId":conversation,"messageId":message,"text":"hello"}),
    )
}

#[test]
fn pause_cancels_queued_turns_and_resume_reactivates() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    create_telegram(&mut state, ChannelId::generate()).unwrap();
    admit(&mut state, WORKSPACE, "first");
    let running = state.ready().unwrap().remove(0);
    let queued = admit(&mut state, WORKSPACE, "second");
    let session = running.conversation.session_id.clone();
    assert_eq!(state.channels().len(), 2);

    let paused = state
        .execute("bot.pause.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();

    assert_eq!(
        paused["bot"]["state"],
        json!({"kind":"paused","reason":"user"})
    );
    assert_eq!(
        bot_state(&mut state),
        json!({"kind":"paused","reason":"user"})
    );
    assert!(running.cancel.is_cancelled());
    assert_eq!(state.turn(queued.id).unwrap().status, TurnStatus::Cancelled);
    assert!(
        state.session_actions.iter().any(
            |(_, action)| matches!(action, Action::Release { agent_id } if agent_id == &session)
        )
    );
    assert_eq!(
        send(&mut state, queued.conversation_id, "third"),
        Err(ErrorCode::BotConflict)
    );
    assert!(state.channels().is_empty());
    assert_eq!(
        state.execute("bot.pause.request", json!({"workspaceId":WORKSPACE})),
        Err(ErrorCode::BotConflict)
    );

    let resumed = state
        .execute("bot.resume.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();

    assert_eq!(resumed["bot"]["state"], json!({"kind":"active"}));
    assert_eq!(bot_state(&mut state), json!({"kind":"active"}));
    assert_eq!(
        send(&mut state, queued.conversation_id, "third").unwrap()["turn"]["status"],
        "queued"
    );
    assert_eq!(state.channels().len(), 2);
    assert_eq!(
        state.execute("bot.resume.request", json!({"workspaceId":WORKSPACE})),
        Err(ErrorCode::BotConflict)
    );
}

#[test]
fn remove_then_setup_keeps_memory_unless_cleared() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let created = state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    remember(&mut state, "prefs");
    create_telegram(&mut state, ChannelId::generate()).unwrap();
    let old = admit(&mut state, WORKSPACE, "first");
    let session = state.conversations[&old.conversation_id].session_id.clone();

    let removed = state
        .execute("bot.remove.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();

    assert_eq!(removed["bot"]["state"]["kind"], "retired");
    assert!(
        state.session_actions.iter().any(
            |(_, action)| matches!(action, Action::Close { agent_id } if agent_id == &session)
        )
    );
    assert_eq!(
        state
            .execute("bot.get.request", json!({"workspaceId":WORKSPACE}))
            .unwrap(),
        json!({"bot":null})
    );
    assert_eq!(state.turn(old.id).unwrap().status, TurnStatus::Cancelled);
    assert!(state.channels().is_empty());
    assert_eq!(
        state.execute("bot.remove.request", json!({"workspaceId":WORKSPACE})),
        Err(ErrorCode::BotNotFound)
    );

    let recreated = state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();

    assert_eq!(recreated["created"], true);
    assert_eq!(recalled(&mut state), "prefs");
    assert_ne!(
        recreated["bot"]["builtinChannelId"],
        created["bot"]["builtinChannelId"]
    );
    assert_eq!(
        send(&mut state, old.conversation_id, "second"),
        Err(ErrorCode::BotNotFound)
    );
    create_telegram(&mut state, ChannelId::generate()).unwrap();

    state
        .execute("bot.remove.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();
    let mut cleared = setup(WORKSPACE);
    cleared["clearMemory"] = json!(true);
    state.execute("bot.setup.request", cleared).unwrap();

    assert_eq!(recalled(&mut state), "");
}

#[test]
fn archive_retires_and_deletes_memory() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    remember(&mut state, "prefs");
    fixture.store.save_memory(OTHER_WORKSPACE, "other").unwrap();

    state.retire(&[WORKSPACE.into()]).unwrap();

    assert_eq!(
        state
            .execute("bot.get.request", json!({"workspaceId":WORKSPACE}))
            .unwrap(),
        json!({"bot":null})
    );
    assert_eq!(fixture.store.memory(WORKSPACE).unwrap(), "");
    assert_eq!(fixture.store.memory(OTHER_WORKSPACE).unwrap(), "other");
}

#[test]
fn missing_directory_pauses_and_cancels_queued_turns() {
    let fixture = Fixture::with_workspace_subdir();
    let directory = fixture.root.path().join("ws");
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    let queued = admit(&mut state, WORKSPACE, "first");
    std::fs::remove_dir(&directory).unwrap();

    let ready = state.ready().unwrap();

    assert!(ready.is_empty());
    assert_eq!(
        bot_state(&mut state),
        json!({"kind":"paused","reason":"directory_missing"})
    );
    assert_eq!(state.turn(queued.id).unwrap().status, TurnStatus::Cancelled);
    assert!(
        !state
            .session_actions
            .iter()
            .any(|(_, action)| matches!(action, Action::Send { .. }))
    );
    assert_eq!(
        send(&mut state, queued.conversation_id, "second"),
        Err(ErrorCode::BotConflict)
    );
    assert_eq!(
        state.execute("bot.resume.request", json!({"workspaceId":WORKSPACE})),
        Err(ErrorCode::WorkspaceNotFound)
    );

    std::fs::create_dir(&directory).unwrap();
    let resumed = state
        .execute("bot.resume.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();

    assert_eq!(resumed["bot"]["state"], json!({"kind":"active"}));
    send(&mut state, queued.conversation_id, "second").unwrap();
    assert_eq!(state.ready().unwrap().len(), 1);
}
