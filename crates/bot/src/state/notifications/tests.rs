use std::time::Instant;

use chrono::{DateTime, TimeDelta, Utc};
use domain::bot::notification::{
    LoggedNotification, Notification, NotificationKind, NotificationLog, WakeStatus,
};
use domain::bot::{DeliveryStatus, Turn, TurnStatus};
use domain::{BotTurnId, ChannelId, ConversationId};
use model::ErrorCode;
use model::bot::Store;
use serde_json::{Value, json};

use super::super::State;
use crate::channels::Incoming;
use crate::service::Notified;
use crate::test_support::{Fixture, OTHER_WORKSPACE, WORKSPACE, setup};

mod crash;

fn notification(id: &str, scope: Option<&str>) -> Notification {
    Notification {
        id: id.to_owned(),
        workspace_id: WORKSPACE.to_owned(),
        scope: scope.map(str::to_owned),
        kind: NotificationKind::AgentFinished,
        agent_id: Some("agent-1".to_owned()),
        schedule_id: None,
        run_id: None,
        terminal_id: None,
        title: "Fix login".to_owned(),
        summary: Some("Tests pass".to_owned()),
        at: Utc::now(),
        conversation_id: None,
    }
}

fn scoped(id: &str) -> Notification {
    notification(id, Some(WORKSPACE))
}

fn active(fixture: &Fixture) -> State {
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    state
}

fn list(state: &State, extra: &Value) -> Value {
    let mut params = json!({"workspaceId":WORKSPACE});
    if let (Some(params), Some(extra)) = (params.as_object_mut(), extra.as_object()) {
        params.extend(extra.clone());
    }
    state
        .list_notifications(&crate::protocol::decode(params).unwrap())
        .unwrap()
}

fn states(state: &State) -> Vec<Value> {
    list(state, &json!({}))["notifications"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["wake"]["state"].clone())
        .collect()
}

/// Every wake turn currently stored, in admission order.
fn wake_turns(fixture: &Fixture) -> Vec<Turn> {
    let mut turns: Vec<Turn> = fixture
        .store
        .conversations()
        .unwrap()
        .into_iter()
        .flat_map(|conversation| fixture.store.turns(conversation.id).unwrap())
        .filter(|turn| turn.wake.is_some())
        .collect();
    turns.sort_by(|left, right| left.created_at.cmp(&right.created_at));
    turns
}

fn owner(state: &mut State) -> ConversationId {
    let opened = state
        .execute(
            "bot.conversation.open.request",
            json!({"workspaceId":WORKSPACE}),
        )
        .unwrap();
    serde_json::from_value(opened["conversation"]["id"].clone()).unwrap()
}

/// Run every ready turn to completion with `reply`.
fn complete_ready(state: &mut State, reply: &str) -> Vec<Turn> {
    let runs = state.ready().unwrap();
    for run in &runs {
        state
            .progress(run.turn.id, TurnStatus::Completed, Some(reply.into()), None)
            .unwrap();
        state.finished(run.conversation.id);
    }
    runs.into_iter()
        .map(|run| state.turn(run.turn.id).unwrap())
        .collect()
}

fn telegram(state: &mut State) -> ChannelId {
    let channel = ChannelId::generate();
    state.execute(
        "bot.channel.create.request",
        json!({"workspaceId":WORKSPACE,"channelId":channel,"enabled":true,"config":{"kind":"telegram","tokenRef":"env:AIT_SERVER_CREDENTIAL_TELEGRAM","allowedSenders":["alice"]}}),
    )
    .unwrap();
    channel
}

/// A Telegram conversation with one completed user turn.
fn telegram_conversation(state: &mut State, channel: ChannelId) -> ConversationId {
    let accepted = state
        .accept(Incoming {
            channel_id: channel,
            peer: "chat".into(),
            sender: "alice".into(),
            thread: None,
            message_id: "m1".into(),
            text: "fix login".into(),
            reply_to: Some("41".into()),
        })
        .unwrap();
    complete_ready(state, "on it");
    serde_json::from_value(accepted["turn"]["conversationId"].clone()).unwrap()
}

fn later(seconds: i64) -> DateTime<Utc> {
    Utc::now() + TimeDelta::seconds(seconds)
}

#[test]
fn unconfigured_and_paused_workspaces_store_nothing() {
    let fixture = Fixture::new();
    let mut state = fixture.state();

    assert_eq!(
        state.notify(scoped("a"), Utc::now()).unwrap(),
        Notified::Ignored
    );
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    state
        .execute("bot.pause.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();
    state.session_actions.clear();

    assert_eq!(
        state.notify(scoped("b"), Utc::now()).unwrap(),
        Notified::Ignored
    );
    state.wake(later(60));
    assert!(fixture.store.notification_logs().unwrap().is_empty());
    assert!(fixture.store.conversations().unwrap().is_empty());
    assert!(state.session_actions.is_empty());
    assert!(!fixture.root.path().join("bots/notifications").exists());
}

#[test]
fn duplicate_ids_are_ignored_and_malformed_identities_rejected() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let mut long = scoped("long");
    long.title = "\u{e9}".repeat(200);

    assert_eq!(state.notify(long, Utc::now()).unwrap(), Notified::Admitted);
    assert_eq!(
        state.notify(scoped("long"), Utc::now()).unwrap(),
        Notified::Duplicate
    );
    let mut invalid = scoped("bad");
    invalid.id = "line\nbreak".into();
    assert_eq!(
        state.notify(invalid, Utc::now()),
        Err(ErrorCode::InvalidMessage)
    );
    let mut invalid = scoped("bad");
    invalid.agent_id = Some(String::new());
    assert_eq!(
        state.notify(invalid, Utc::now()),
        Err(ErrorCode::InvalidMessage)
    );

    let logs = fixture.store.notification_logs().unwrap();
    assert_eq!(logs[0].entries.len(), 1);
    assert_eq!(logs[0].entries[0].notification.title.len(), 256);
}

#[test]
fn own_scope_wakes_only_scoped_subjects_and_all_wakes_every_workspace_event() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);

    state
        .notify(notification("user", None), Utc::now())
        .unwrap();
    state.notify(scoped("bot"), Utc::now()).unwrap();
    let mut elsewhere = notification("other", None);
    elsewhere.workspace_id = OTHER_WORKSPACE.into();
    assert_eq!(
        state.notify(elsewhere, Utc::now()).unwrap(),
        Notified::Ignored
    );
    assert_eq!(states(&state), [json!("pending"), json!("logged")]);

    let mut update = setup(WORKSPACE);
    update["wakeScope"] = json!("all");
    let updated = state.execute("bot.update.request", update).unwrap();
    assert_eq!(updated["bot"]["wakeScope"], "all");
    state
        .notify(notification("user-2", None), Utc::now())
        .unwrap();

    assert_eq!(
        states(&state),
        [json!("pending"), json!("pending"), json!("logged")]
    );
    let got = state
        .execute("bot.get.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();
    assert_eq!(got["bot"]["wakeScope"], "all");
    assert_eq!(list(&state, &json!({}))["wakeScope"], "all");
}

#[test]
fn setup_records_the_wake_scope_and_compares_it_for_idempotency() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let mut all = setup(WORKSPACE);
    all["wakeScope"] = json!("all");

    let created = state.execute("bot.setup.request", all.clone()).unwrap();

    assert_eq!(created["bot"]["wakeScope"], "all");
    assert_eq!(
        state.execute("bot.setup.request", all).unwrap()["created"],
        false
    );
    assert_eq!(
        state.execute("bot.setup.request", setup(WORKSPACE)),
        Err(ErrorCode::BotConflict)
    );
}

#[test]
fn pending_notifications_coalesce_into_one_wake_turn_after_quiet() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let start = Utc::now();
    for (index, offset) in [0, 1, 2].into_iter().enumerate() {
        state
            .notify(
                scoped(&format!("n{index}")),
                start + TimeDelta::seconds(offset),
            )
            .unwrap();
    }

    state.wake(start + TimeDelta::seconds(4));
    assert!(wake_turns(&fixture).is_empty());
    state.wake(start + TimeDelta::seconds(5));

    let turns = wake_turns(&fixture);
    assert_eq!(turns.len(), 1);
    let turn = &turns[0];
    assert_eq!(turn.wake.as_ref().unwrap().notifications.len(), 3);
    assert_eq!(turn.reply_to, None);
    assert_eq!(turn.message_id, format!("notification:{}", turn.id));
    assert!(
        turn.text
            .starts_with("<ait-notifications>\n- agent_finished at ")
    );
    assert!(
        turn.text
            .contains("Fix login [agent agent-1]\n  Tests pass")
    );
    assert_eq!(turn.conversation_id, owner(&mut state));
    assert_eq!(
        states(&state),
        [json!("woke"), json!("woke"), json!("woke")]
    );
    state.wake(start + TimeDelta::seconds(60));
    assert_eq!(wake_turns(&fixture).len(), 1);
}

#[test]
fn a_steady_stream_still_wakes_after_the_longest_delay() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let start = Utc::now();
    for second in (0..=30).step_by(2) {
        let now = start + TimeDelta::seconds(second);
        state.notify(scoped(&format!("s{second}")), now).unwrap();
        state.wake(now);
    }

    assert_eq!(wake_turns(&fixture).len(), 1);
    assert_eq!(
        wake_turns(&fixture)[0]
            .wake
            .as_ref()
            .unwrap()
            .notifications
            .len(),
        16
    );
}

#[test]
fn notifications_accumulate_while_the_conversation_runs_and_fire_after_it() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let conversation = owner(&mut state);
    state
        .execute(
            "bot.message.send.request",
            json!({"workspaceId":WORKSPACE,"conversationId":conversation,"messageId":"u1","text":"hello"}),
        )
        .unwrap();
    let runs = state.ready().unwrap();
    state.notify(scoped("a"), Utc::now()).unwrap();
    state.notify(scoped("b"), Utc::now()).unwrap();

    state.wake(later(60));
    assert!(wake_turns(&fixture).is_empty());
    state
        .progress(
            runs[0].turn.id,
            TurnStatus::Completed,
            Some("done".into()),
            None,
        )
        .unwrap();
    state.finished(conversation);
    state.wake(later(60));

    let turns = wake_turns(&fixture);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].conversation_id, conversation);
    assert_eq!(turns[0].wake.as_ref().unwrap().notifications.len(), 2);
}

#[test]
fn wake_turns_go_to_the_origin_conversation_or_fall_back_to_the_owner() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let channel = telegram(&mut state);
    let origin = telegram_conversation(&mut state, channel);
    let mut from_origin = scoped("origin");
    from_origin.conversation_id = Some(origin);
    let mut unknown = scoped("unknown");
    unknown.conversation_id = Some(ConversationId::generate());

    state.notify(from_origin, Utc::now()).unwrap();
    state.notify(unknown, Utc::now()).unwrap();
    state.wake(later(10));

    let turns = wake_turns(&fixture);
    assert_eq!(turns.len(), 2);
    let owner = owner(&mut state);
    let mut targets: Vec<_> = turns.iter().map(|turn| turn.conversation_id).collect();
    targets.sort();
    let mut expected = vec![origin, owner];
    expected.sort();
    assert_eq!(targets, expected);

    complete_ready(&mut state, "reported");
    state
        .execute(
            "bot.channel.update.request",
            json!({"workspaceId":WORKSPACE,"channelId":channel,"enabled":false}),
        )
        .unwrap();
    let mut disabled = scoped("disabled");
    disabled.conversation_id = Some(origin);
    state.notify(disabled, Utc::now()).unwrap();
    state.wake(later(10));

    assert_eq!(wake_turns(&fixture).last().unwrap().conversation_id, owner);
}

#[test]
fn an_exhausted_wake_budget_keeps_notifications_in_the_log_only() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let start = Utc::now();
    for index in 0..12 {
        let now = start + TimeDelta::seconds(index * 10);
        state.notify(scoped(&format!("w{index}")), now).unwrap();
        state.wake(now + TimeDelta::seconds(5));
        complete_ready(&mut state, "reported");
    }
    assert_eq!(wake_turns(&fixture).len(), 12);

    let now = start + TimeDelta::seconds(200);
    state.notify(scoped("over"), now).unwrap();
    state.wake(now + TimeDelta::seconds(5));

    assert_eq!(wake_turns(&fixture).len(), 12);
    let newest = &list(&state, &json!({"limit":1}))["notifications"][0];
    assert_eq!(
        newest["wake"],
        json!({"state":"skipped","reason":"over_budget"})
    );

    let after_window = start + TimeDelta::hours(2);
    state.notify(scoped("fresh"), after_window).unwrap();
    state.wake(after_window + TimeDelta::seconds(5));
    assert_eq!(wake_turns(&fixture).len(), 13);
}

#[test]
fn a_restart_keeps_the_pending_inbox_and_never_repeats_a_committed_wake() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    state.notify(scoped("a"), Utc::now()).unwrap();
    drop(state);

    let mut restarted = fixture.state();
    assert_eq!(states(&restarted), [json!("pending")]);
    let before_wake = fixture.store.notification_logs().unwrap().remove(0);
    restarted.wake(later(10));
    assert_eq!(wake_turns(&fixture).len(), 1);
    // A crash after the wake turn was committed but before the log was saved.
    fixture.store.save_notification_log(&before_wake).unwrap();
    drop(restarted);

    let mut recovered = fixture.state();
    let turn = wake_turns(&fixture).remove(0);
    assert_eq!(
        fixture.store.notification_logs().unwrap()[0].entries[0].wake,
        WakeStatus::Woke {
            conversation_id: turn.conversation_id,
            turn_id: turn.id,
        }
    );
    recovered.wake(later(60));
    assert_eq!(wake_turns(&fixture).len(), 1);
    assert_eq!(recovered.wakes[&turn.conversation_id].len(), 1);
}

#[test]
fn a_silent_wake_reply_is_marked_delivered_without_sending() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let channel = telegram(&mut state);
    let origin = telegram_conversation(&mut state, channel);
    while let Some((turn, _)) = state.pending_delivery(channel).unwrap() {
        state.delivery(turn.id, DeliveryStatus::Delivered).unwrap();
    }
    let mut from_origin = scoped("quiet");
    from_origin.conversation_id = Some(origin);
    state.notify(from_origin, Utc::now()).unwrap();
    state.wake(later(10));

    let turn = complete_ready(&mut state, "  NO_REPLY\n").remove(0);

    assert!(turn.silent);
    assert_eq!(turn.delivery, DeliveryStatus::Delivered);
    assert!(state.pending_delivery(channel).unwrap().is_none());
    let accepted = state
        .accept(Incoming {
            channel_id: channel,
            peer: "chat".into(),
            sender: "alice".into(),
            thread: None,
            message_id: "m2".into(),
            text: "anything?".into(),
            reply_to: None,
        })
        .unwrap();
    let user = complete_ready(&mut state, "NO_REPLY").remove(0);
    assert_eq!(user.id.to_string(), accepted["turn"]["id"]);
    assert!(!user.silent);
    assert_eq!(
        state.pending_delivery(channel).unwrap().unwrap().0.id,
        user.id
    );
}

#[test]
fn unseen_notifications_advance_only_with_completed_turns() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let conversation = owner(&mut state);
    state
        .notify(notification("first", None), Utc::now())
        .unwrap();
    let send = |state: &mut State, message: &str| {
        state
            .execute(
                "bot.message.send.request",
                json!({"workspaceId":WORKSPACE,"conversationId":conversation,"messageId":message,"text":"hello"}),
            )
            .unwrap();
    };
    let unseen = |state: &State| {
        list(state, &json!({"unseenBy":conversation}))["notifications"]
            .as_array()
            .unwrap()
            .len()
    };

    send(&mut state, "u1");
    let run = state.ready().unwrap().remove(0);
    assert_eq!(
        state.turn(run.turn.id).unwrap().notification_cursor,
        Some(1)
    );
    state
        .progress(run.turn.id, TurnStatus::Failed, None, Some("failed".into()))
        .unwrap();
    state.finished(conversation);
    assert_eq!(unseen(&state), 1);

    send(&mut state, "u2");
    complete_ready(&mut state, "done");
    assert_eq!(unseen(&state), 0);
    state
        .notify(notification("second", None), Utc::now())
        .unwrap();

    let page = list(&state, &json!({"unseenBy":conversation}));
    assert_eq!(page["headSequence"], 2);
    assert_eq!(page["seenSequence"], 1);
    assert_eq!(page["notifications"][0]["notification"]["id"], "second");
    drop(state);
    assert_eq!(unseen(&fixture.state()), 1);
    assert_eq!(
        fixture.state().list_notifications(
            &crate::protocol::decode(json!({"workspaceId":WORKSPACE,"limit":0})).unwrap()
        ),
        Err(ErrorCode::InvalidMessage)
    );
}

#[test]
fn pause_drops_the_inbox_and_retirement_removes_the_notification_state() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    state.notify(scoped("a"), Utc::now()).unwrap();
    state.notify(notification("b", None), Utc::now()).unwrap();

    state
        .execute("bot.pause.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();
    state
        .execute("bot.resume.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();
    state.wake(later(60));

    assert!(wake_turns(&fixture).is_empty());
    assert_eq!(
        list(&state, &json!({}))["notifications"][1]["wake"],
        json!({"state":"skipped","reason":"paused"})
    );
    state
        .execute("bot.remove.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();
    assert!(fixture.store.notification_logs().unwrap().is_empty());
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    assert_eq!(list(&state, &json!({}))["headSequence"], 0);
}

#[test]
fn a_full_inbox_skips_new_notifications() {
    let fixture = Fixture::new();
    let bot = active(&fixture).current(WORKSPACE).unwrap().id;
    let mut log = NotificationLog::new(bot);
    log.entries = (1..=64)
        .map(|sequence| entry(sequence, WakeStatus::Pending))
        .collect();
    log.next_sequence = 65;
    fixture.store.save_notification_log(&log).unwrap();
    let mut state = fixture.state();

    state.notify(scoped("full"), Utc::now()).unwrap();

    assert_eq!(
        list(&state, &json!({"limit":1}))["notifications"][0]["wake"],
        json!({"state":"skipped","reason":"inbox_full"})
    );
}

#[test]
fn eviction_drops_only_the_oldest_settled_entries() {
    let mut entries: Vec<LoggedNotification> = (1..=200)
        .map(|sequence| {
            let wake = if sequence % 4 == 0 {
                WakeStatus::Pending
            } else {
                WakeStatus::Logged
            };
            entry(sequence, wake)
        })
        .collect();

    super::evict(&mut entries);

    let settled: Vec<u64> = entries
        .iter()
        .filter(|entry| entry.wake == WakeStatus::Logged)
        .map(|entry| entry.sequence)
        .collect();
    assert_eq!(settled.len(), 128);
    assert_eq!(settled[0], 30);
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.wake == WakeStatus::Pending)
            .count(),
        50
    );
}

fn entry(sequence: u64, wake: WakeStatus) -> LoggedNotification {
    LoggedNotification {
        sequence,
        admitted_at: Utc::now(),
        notification: scoped(&format!("e{sequence}")),
        wake,
    }
}

/// Put `conversation` in the state a daemon crash leaves it in: its current generation holds an
/// uncertain turn, so nothing runs there until the user resets it.
fn make_uncertain(state: &mut State, conversation: ConversationId) {
    state
        .execute(
            "bot.message.send.request",
            json!({"workspaceId":WORKSPACE,"conversationId":conversation,"messageId":"stuck","text":"hello"}),
        )
        .unwrap();
    let run = state.ready().unwrap().remove(0);
    state
        .progress(
            run.turn.id,
            TurnStatus::Uncertain,
            None,
            Some("lost".into()),
        )
        .unwrap();
    state.finished(conversation);
}

fn from(id: &str, origin: ConversationId) -> Notification {
    let mut notification = scoped(id);
    notification.conversation_id = Some(origin);
    notification
}

fn wake_state(state: &State, id: &str) -> Value {
    list(state, &json!({"limit":128}))["notifications"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["notification"]["id"] == id)
        .map(|entry| entry["wake"].clone())
        .unwrap()
}

#[test]
fn an_uncertain_target_settles_its_notifications_as_blocked_and_starves_no_one() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let channel = telegram(&mut state);
    let origin = telegram_conversation(&mut state, channel);
    let owner = owner(&mut state);
    make_uncertain(&mut state, owner);
    let start = Utc::now();
    for index in 0..64 {
        state
            .notify(scoped(&format!("held{index}")), start)
            .unwrap();
    }

    state.wake(start + TimeDelta::seconds(10));
    state.notify(from("healthy", origin), start).unwrap();
    state.wake(start + TimeDelta::seconds(20));

    assert_eq!(
        wake_state(&state, "held0"),
        json!({"state":"skipped","reason":"blocked"})
    );
    let turns = wake_turns(&fixture);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].conversation_id, origin);
    assert_eq!(wake_state(&state, "healthy")["state"], "woke");
}

/// Wake `notification`'s target and start the wake turn, returning its id.
fn start_wake(state: &mut State, notification: Notification) -> BotTurnId {
    state.notify(notification, Utc::now()).unwrap();
    state.wake(later(10));
    let run = state.ready().unwrap().remove(0);
    assert!(run.turn.wake.is_some());
    run.turn.id
}

#[test]
fn a_wake_turn_cancelled_by_shutdown_is_retried_once_per_interruption() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let mut turn = start_wake(&mut state, scoped("a"));

    for round in 1..=2 {
        // The service is stopping: `serve` cancels every running turn.
        state.stopping = true;
        state
            .progress(turn, TurnStatus::Cancelled, None, None)
            .unwrap();
        let conversation = state.turn(turn).unwrap().conversation_id;
        state.finished(conversation);
        drop(state);

        state = fixture.state();
        assert_eq!(
            wake_state(&state, "a"),
            json!({"state":"pending"}),
            "{round}"
        );
        state.wake(later(10));
        assert_eq!(wake_turns(&fixture).len(), round + 1);
        turn = state.ready().unwrap().remove(0).turn.id;
    }

    complete_ready(&mut state, "reported");
    assert_eq!(wake_state(&state, "a")["turnId"], json!(turn));
    let log = &fixture.store.notification_logs().unwrap()[0];
    assert_eq!(log.entries.len(), 1);
    assert!(
        wake_turns(&fixture)
            .iter()
            .all(|turn| turn.wake.as_ref().unwrap().notifications.len() == 1)
    );
}

#[test]
fn a_wake_turn_the_user_cancelled_is_not_retried() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let turn = start_wake(&mut state, scoped("a"));

    state
        .progress(turn, TurnStatus::Cancelled, None, None)
        .unwrap();
    let conversation = state.turn(turn).unwrap().conversation_id;
    state.finished(conversation);
    drop(state);
    let mut restarted = fixture.state();
    restarted.wake(later(60));

    assert_eq!(wake_state(&restarted, "a")["state"], "woke");
    assert_eq!(wake_turns(&fixture).len(), 1);
}

#[test]
fn a_wake_turn_a_crash_interrupted_is_retried_in_the_owner_conversation() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let channel = telegram(&mut state);
    let origin = telegram_conversation(&mut state, channel);
    start_wake(&mut state, from("a", origin));
    // A crash: the turn stays `sending` on disk.
    drop(state);

    let mut restarted = fixture.state();
    assert_eq!(wake_state(&restarted, "a"), json!({"state":"pending"}));
    restarted.wake(later(10));

    let turns = wake_turns(&fixture);
    assert_eq!(turns.len(), 2);
    let owner = owner(&mut restarted);
    assert_eq!(turns[1].conversation_id, owner);
    assert_eq!(wake_state(&restarted, "a")["conversationId"], json!(owner));
}

#[test]
fn a_replayed_notification_that_already_woke_stays_a_duplicate_after_eviction() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    state.notify(scoped("a"), Utc::now()).unwrap();
    state.wake(later(10));
    complete_ready(&mut state, "reported");
    for index in 0..128 {
        state
            .notify(notification(&format!("noise{index}"), None), Utc::now())
            .unwrap();
    }
    assert_eq!(
        state.notify(scoped("a"), Utc::now()).unwrap(),
        Notified::Duplicate
    );
    drop(state);

    let mut restarted = fixture.state();
    assert_eq!(
        restarted.notify(scoped("a"), later(300)).unwrap(),
        Notified::Duplicate
    );
    restarted.wake(later(320));
    assert_eq!(wake_turns(&fixture).len(), 1);
}

#[test]
fn an_agent_the_conversation_observed_does_not_wake_it_again() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let channel = telegram(&mut state);
    let elsewhere = telegram_conversation(&mut state, channel);
    let conversation = owner(&mut state);
    state
        .execute(
            "bot.message.send.request",
            json!({"workspaceId":WORKSPACE,"conversationId":conversation,"messageId":"u1","text":"fix it"}),
        )
        .unwrap();
    let run = state.ready().unwrap().remove(0);
    let finished_at = Utc::now();
    let agent = |id: &str, agent: &str, origin: ConversationId, at: DateTime<Utc>| {
        let mut notification = from(id, origin);
        notification.agent_id = Some(agent.to_owned());
        notification.at = at;
        notification
    };
    state
        .notify(
            agent("seen", "agent-1", conversation, finished_at),
            Utc::now(),
        )
        .unwrap();
    state
        .notify(
            agent("other", "agent-2", conversation, finished_at),
            Utc::now(),
        )
        .unwrap();
    state
        .notify(
            agent("there", "agent-1", elsewhere, finished_at),
            Utc::now(),
        )
        .unwrap();

    // The running turn's `wait_for_agent` saw agent-1 settled at the finish.
    state.observe(conversation, "agent-1", finished_at, Instant::now());
    let rerun = agent(
        "again",
        "agent-1",
        conversation,
        finished_at + TimeDelta::seconds(1),
    );
    state.notify(rerun, Utc::now()).unwrap();
    state
        .progress(
            run.turn.id,
            TurnStatus::Completed,
            Some("done".into()),
            None,
        )
        .unwrap();
    state.finished(conversation);
    state.wake(later(10));

    assert_eq!(
        wake_state(&state, "seen"),
        json!({"state":"skipped","reason":"observed"})
    );
    let turns = wake_turns(&fixture);
    let carried = |target: ConversationId| -> Vec<String> {
        turns
            .iter()
            .filter(|turn| turn.conversation_id == target)
            .flat_map(|turn| turn.wake.as_ref().unwrap().notifications.clone())
            .map(|notification| notification.id)
            .collect()
    };
    assert_eq!(carried(conversation), ["other", "again"]);
    assert_eq!(carried(elsewhere), ["there"]);
}

/// Send `text` as the owner in `conversation` and start that turn, returning its id.
fn user_turn(state: &mut State, conversation: ConversationId, message: &str) -> BotTurnId {
    state
        .execute(
            "bot.message.send.request",
            json!({"workspaceId":WORKSPACE,"conversationId":conversation,"messageId":message,"text":"fix it"}),
        )
        .unwrap();
    let run = state.ready().unwrap().remove(0);
    assert!(run.turn.wake.is_none());
    run.turn.id
}

#[test]
fn an_observation_by_a_turn_that_does_not_complete_never_suppresses_the_report() {
    for end in [TurnStatus::Failed, TurnStatus::Cancelled] {
        let fixture = Fixture::new();
        let mut state = active(&fixture);
        let conversation = owner(&mut state);
        let turn = user_turn(&mut state, conversation, "u1");
        let report = from("seen", conversation);
        let finished_at = report.at;
        state.notify(report, Utc::now()).unwrap();

        // The running turn's `wait_for_agent` saw agent-1 settled; it has not replied yet.
        state.observe(conversation, "agent-1", finished_at, Instant::now());
        state.wake(later(10));
        assert_eq!(
            wake_state(&state, "seen"),
            json!({"state":"pending"}),
            "{end:?}"
        );
        state.progress(turn, end, None, None).unwrap();
        state.finished(conversation);
        state.wake(later(20));

        assert_eq!(wake_state(&state, "seen")["state"], "woke", "{end:?}");
        assert_eq!(wake_turns(&fixture).len(), 1, "{end:?}");
    }
}

#[test]
fn a_late_read_of_a_cancelled_turn_never_suppresses_the_report_for_the_next_turn() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let conversation = owner(&mut state);
    let cancelled = user_turn(&mut state, conversation, "u1");
    // The first turn's `wait_for_agent` call opens, then the user cancels that turn.
    let read_started = Instant::now();
    state
        .progress(cancelled, TurnStatus::Cancelled, None, None)
        .unwrap();
    state.finished(conversation);
    let next = user_turn(&mut state, conversation, "u2");
    let report = from("seen", conversation);
    let finished_at = report.at;
    state.notify(report, Utc::now()).unwrap();

    // The cancelled turn's call returns while the next turn, on another subject, runs.
    state.observe(conversation, "agent-1", finished_at, read_started);
    state
        .progress(next, TurnStatus::Completed, Some("other".into()), None)
        .unwrap();
    state.finished(conversation);
    state.wake(later(10));

    assert_eq!(wake_state(&state, "seen")["state"], "woke");
    assert_eq!(wake_turns(&fixture).len(), 1);
}

#[test]
fn an_observation_by_a_turn_a_restart_cut_short_never_suppresses_the_report() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let channel = telegram(&mut state);
    let conversation = telegram_conversation(&mut state, channel);
    state
        .accept(Incoming {
            channel_id: channel,
            peer: "chat".into(),
            sender: "alice".into(),
            thread: None,
            message_id: "m2".into(),
            text: "is it done?".into(),
            reply_to: Some("42".into()),
        })
        .unwrap();
    state.ready().unwrap();
    let report = from("seen", conversation);
    let finished_at = report.at;
    state.notify(report, Utc::now()).unwrap();
    state.observe(conversation, "agent-1", finished_at, Instant::now());
    state.wake(later(10));
    // A crash before the observing turn replied.
    drop(state);

    let mut restarted = fixture.state();
    restarted.wake(later(20));

    // The interrupted conversation is uncertain, so the report falls back to the owner.
    assert_eq!(wake_state(&restarted, "seen")["state"], "woke");
    assert_eq!(wake_turns(&fixture).len(), 1);
}

#[test]
fn observations_stay_bounded_and_need_a_running_turn() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let conversation = owner(&mut state);
    // Newest of all, so only the missing turn can keep it out.
    state.observe(conversation, "outside", later(3600), Instant::now());
    let turn = user_turn(&mut state, conversation, "u1");

    for index in 0..1000 {
        state.observe(
            conversation,
            &format!("agent-{index}"),
            Utc::now(),
            Instant::now(),
        );
    }
    state
        .progress(turn, TurnStatus::Completed, Some("done".into()), None)
        .unwrap();
    state.finished(conversation);

    assert!(state.observing.is_empty());
    assert!(state.observations.len() <= 256);
    let seen = |agent: &str| {
        state
            .observations
            .contains_key(&(conversation, agent.to_owned()))
    };
    assert!(seen("agent-999"));
    assert!(!seen("outside"));
}

#[test]
fn updating_only_the_wake_scope_keeps_the_execution() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let before = state
        .execute("bot.get.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();

    let updated = state
        .execute(
            "bot.update.request",
            json!({"workspaceId":WORKSPACE,"wakeScope":"all"}),
        )
        .unwrap();

    assert_eq!(updated["bot"]["wakeScope"], "all");
    assert_eq!(updated["bot"]["execution"], before["bot"]["execution"]);
}
