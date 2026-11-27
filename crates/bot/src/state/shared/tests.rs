use super::*;
use crate::test_support::{Fixture, WORKSPACE, setup};
use domain::ChannelId;
use domain::bot::TurnStatus;

fn channel(state: &mut State) -> ChannelId {
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    let id = ChannelId::generate();
    state.execute("bot.channel.create.request", json!({"workspaceId":WORKSPACE,"channelId":id,"enabled":true,
        "config":{"kind":"feishu","appId":"app","secretRef":"env:AIT_SERVER_CREDENTIAL_TEST","allowGroups":true}})).unwrap();
    id
}

fn input(channel: ChannelId, peer: &str, message: &str) -> Incoming {
    Incoming {
        channel_id: channel,
        peer: peer.into(),
        sender: "user".into(),
        thread: None,
        message_id: message.into(),
        text: message.into(),
        reply_to: Some(message.into()),
        passive: false,
        sender_type: Some("user".into()),
        context: None,
    }
}

#[test]
fn bot_serializes_sources_on_one_runtime_and_keeps_delivery_routes() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let channel = channel(&mut state);
    let a = state.accept(input(channel, "group-a", "a")).unwrap();
    let b = state.accept(input(channel, "group-b", "b")).unwrap();
    let first = state.ready().unwrap().pop().unwrap();
    assert_eq!(first.turn.id.to_string(), a["turn"]["id"].as_str().unwrap());
    assert!(state.ready().unwrap().is_empty());
    let session = first.conversation.session_id.clone();
    let owner = first.owner;
    state
        .progress(
            first.turn.id,
            TurnStatus::Completed,
            Some("done-a".into()),
            None,
        )
        .unwrap();
    state.finished(first.turn.conversation_id);
    let second = state.ready().unwrap().pop().unwrap();
    assert_eq!(
        second.turn.id.to_string(),
        b["turn"]["id"].as_str().unwrap()
    );
    assert_eq!(second.conversation.session_id, session);
    assert_eq!(second.owner, owner);
    assert_ne!(first.turn.conversation_id, second.turn.conversation_id);
    let delivered = state.pending_delivery(channel).unwrap().unwrap();
    assert_eq!(delivered.1.peer, "group-a");
    assert_eq!(delivered.0.reply_to.as_deref(), Some("a"));
}

#[test]
fn reception_is_per_chat_and_private_chat_needs_no_mention() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let channel = channel(&mut state);
    assert_eq!(
        state
            .admit_chat(channel, "group-a".into(), true, false)
            .unwrap()["accepted"],
        false
    );
    state.execute("bot.chat.update.request",json!({"workspaceId":WORKSPACE,"channelId":channel,"peer":"group-a","enabled":true,"listenAllMessages":true})).unwrap();
    assert_eq!(
        state
            .admit_chat(channel, "group-a".into(), true, false)
            .unwrap()["accepted"],
        true
    );
    assert_eq!(
        state
            .admit_chat(channel, "group-b".into(), true, false)
            .unwrap()["accepted"],
        false
    );
    assert_eq!(
        state
            .admit_chat(channel, "private".into(), false, false)
            .unwrap()["accepted"],
        true
    );
    assert!(state.conversations.is_empty());
    let restarted = fixture.state();
    assert_eq!(restarted.catalog.chats.len(), 3);
}

#[test]
fn passive_message_can_complete_without_outbound_delivery() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let channel = channel(&mut state);
    let mut message = input(channel, "group-a", "a");
    message.passive = true;
    let admitted = state.accept(message.clone()).unwrap();
    let run = state.ready().unwrap().pop().unwrap();
    state
        .progress(
            run.turn.id,
            TurnStatus::Completed,
            Some("NO_REPLY".into()),
            None,
        )
        .unwrap();
    assert!(state.turn(run.turn.id).unwrap().silent);
    assert!(state.pending_delivery(channel).unwrap().is_none());
    assert_eq!(state.accept(message).unwrap()["duplicate"], true);
    assert_eq!(admitted["turn"]["id"], json!(run.turn.id));
}

#[test]
fn legacy_feishu_receipt_is_deduplicated_by_platform_message_after_restart() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let channel = channel(&mut state);
    let mut old = input(channel, "group-a", "event-old");
    old.reply_to = Some("om_original".into());
    let accepted = state.accept(old).unwrap();
    drop(state);
    let mut restarted = fixture.state();
    let mut replay = input(channel, "group-a", "om_original");
    replay.text = "event-old".into();
    assert_eq!(
        restarted.accept(replay).unwrap()["turn"]["id"],
        accepted["turn"]["id"]
    );
    assert_eq!(restarted.receipts.len(), 1);
}

#[test]
fn shared_runtime_survives_restart_and_builtin_joins_external_context() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let channel = channel(&mut state);
    state.accept(input(channel, "group-a", "a")).unwrap();
    let first = state.ready().unwrap().pop().unwrap();
    let runtime = first.conversation.session_id.clone();
    state
        .progress(
            first.turn.id,
            TurnStatus::Completed,
            Some("done".into()),
            None,
        )
        .unwrap();
    state.finished(first.turn.conversation_id);
    drop(state);
    let mut state = fixture.state();
    let opened = state
        .open(&WorkspaceRequest {
            workspace_id: WORKSPACE.into(),
        })
        .unwrap();
    state.execute("bot.message.send.request",json!({"workspaceId":WORKSPACE,"conversationId":opened["conversation"]["id"],"messageId":"owner-question","text":"What happened in group-a?"})).unwrap();
    let run = state.ready().unwrap().pop().unwrap();
    assert_eq!(run.conversation.session_id, runtime);
    assert_eq!(run.conversation.peer, "owner");
    assert_eq!(run.turn.bot_sequence, Some(2));
}

#[test]
fn asynchronous_wake_retains_the_origin_thread_after_another_chat_runs() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let channel = channel(&mut state);
    let mut a = input(channel, "group-a", "om_original");
    a.thread = Some("omt_origin".into());
    state.accept(a).unwrap();
    let first = state.ready().unwrap().pop().unwrap();
    state
        .progress(
            first.turn.id,
            TurnStatus::Completed,
            Some("working".into()),
            None,
        )
        .unwrap();
    state.finished(first.turn.conversation_id);
    state
        .accept(input(channel, "group-b", "om_question"))
        .unwrap();
    let second = state.ready().unwrap().pop().unwrap();
    state
        .progress(
            second.turn.id,
            TurnStatus::Completed,
            Some("still working".into()),
            None,
        )
        .unwrap();
    state.finished(second.turn.conversation_id);
    let now = chrono::Utc::now();
    let event = serde_json::from_value(json!({"id":"agent:done","workspaceId":WORKSPACE,"scope":WORKSPACE,
        "kind":"agent_finished","agentId":"worker","title":"Work finished","at":now,"conversationId":first.turn.conversation_id})).unwrap();
    state.notify(event, now).unwrap();
    state.wake(now + chrono::TimeDelta::seconds(31));
    let wake = state.ready().unwrap().pop().unwrap();
    assert_eq!(wake.conversation.peer, "group-a");
    assert_eq!(wake.conversation.thread.as_deref(), Some("omt_origin"));
    assert_eq!(wake.turn.reply_to.as_deref(), Some("om_original"));
    assert_eq!(wake.conversation.session_id, second.conversation.session_id);
}
