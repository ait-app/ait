use super::*;
use domain::bot::DeliveryStatus;
use provider::service::bot_execution::Action;

fn admit(state: &mut super::super::State, bot: BotId, message: &str) -> domain::bot::Turn {
    let opened = state
        .execute("bot.conversation.open.request", json!({"botId":bot}))
        .unwrap();
    let accepted = state.execute("bot.message.send.request", json!({"botId":bot,"conversationId":opened["conversation"]["id"],"messageId":message,"text":"hello"})).unwrap();
    serde_json::from_value(accepted["turn"].clone()).unwrap()
}

#[test]
fn queue_budget_and_reset_conflicts_leave_the_active_turn_untouched() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let bot = BotId::generate();
    state.execute("bot.create.request", creation(bot)).unwrap();
    let first = admit(&mut state, bot, "first");
    let run = state.ready().unwrap().remove(0);
    assert_eq!(
        state.execute(
            "bot.conversation.reset.request",
            json!({"botId":bot,"conversationId":first.conversation_id})
        ),
        Err(ErrorCode::BotConflict)
    );
    assert!(!run.cancel.is_cancelled());
    for index in 0..32 {
        admit(&mut state, bot, &format!("queued-{index}"));
    }
    assert_eq!(state.execute("bot.message.send.request", json!({"botId":bot,"conversationId":first.conversation_id,"messageId":"overflow","text":"hello"})), Err(ErrorCode::ResourceExhausted));
    assert_eq!(
        fixture.store.turns(first.conversation_id).unwrap().len(),
        33
    );
    assert_eq!(admit(&mut state, bot, "first").id, first.id);
    assert!(state.ready().unwrap().is_empty());
}

#[test]
fn pause_preserves_session_identity_and_archive_retires_it() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let bot = BotId::generate();
    state.execute("bot.create.request", creation(bot)).unwrap();
    let first = admit(&mut state, bot, "first");
    let session = fixture.store.conversations().unwrap().remove(0).session_id;
    state
        .execute("bot.update.request", json!({"botId":bot,"enabled":false}))
        .unwrap();
    assert_eq!(state.turn(first.id).unwrap().status, TurnStatus::Cancelled);
    assert!(
        matches!(&state.session_actions[0].1, Action::Release { agent_id } if agent_id == &session)
    );
    state
        .execute("bot.update.request", json!({"botId":bot,"enabled":true}))
        .unwrap();
    let next = admit(&mut state, bot, "second");
    let ready = state.ready().unwrap().remove(0);
    assert_eq!(ready.turn.id, next.id);
    assert_eq!(ready.conversation.session_id, session);
    state
        .execute("bot.archive.request", json!({"botId":bot}))
        .unwrap();
    assert!(ready.cancel.is_cancelled());
    assert!(
        matches!(&state.session_actions[1].1, Action::Close { agent_id } if agent_id == &session)
    );
}

#[test]
fn unreadable_memory_fails_only_its_bot_and_does_not_start_native_execution() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let broken = BotId::generate();
    let healthy = BotId::generate();
    state
        .execute("bot.create.request", creation(broken))
        .unwrap();
    state
        .execute("bot.create.request", creation(healthy))
        .unwrap();
    let failed = admit(&mut state, broken, "first");
    let successful = admit(&mut state, healthy, "first");
    let path = fixture
        .root
        .path()
        .join(model::bot::memory_path("wks_0123456789abcdef", broken));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, vec![b'a'; 32769]).unwrap();
    let ready = state.ready().unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].turn.id, successful.id);
    assert_eq!(state.turn(failed.id).unwrap().status, TurnStatus::Failed);
    assert!(!state.frozen);
}

#[test]
fn recovered_outbox_resumes_only_replies_that_never_started_sending() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let bot = BotId::generate();
    state.execute("bot.create.request", creation(bot)).unwrap();
    let channel = ChannelId::generate();
    state.execute("bot.channel.create.request", json!({"botId":bot,"channelId":channel,"enabled":true,"config":{"kind":"telegram","tokenRef":"env:AIT_SERVER_CREDENTIAL_TELEGRAM","allowedSenders":["alice"]}})).unwrap();
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
    let bot = BotId::generate();
    let channel = ChannelId::generate();
    state.execute("bot.create.request", creation(bot)).unwrap();
    state.execute("bot.channel.create.request", json!({"botId":bot,"channelId":channel,"enabled":true,"config":{"kind":"telegram","tokenRef":"env:AIT_SERVER_CREDENTIAL_TELEGRAM","allowedSenders":["alice"]}})).unwrap();
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
