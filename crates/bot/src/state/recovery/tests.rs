use domain::bot::{DeliveryStatus, TurnStatus};
use domain::{BotTurnId, ChannelId};
use serde_json::json;

use super::*;
use crate::channels::Incoming;
use crate::protocol::WorkspaceRequest;
use crate::test_support::{Fixture, OTHER_WORKSPACE, WORKSPACE, setup};

fn active(fixture: &Fixture) -> (State, ChannelId) {
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    let channel = ChannelId::generate();
    state
        .execute(
            "bot.channel.create.request",
            json!({"workspaceId":WORKSPACE,
        "channelId":channel,"enabled":true,"config":{"kind":"feishu","appId":"app",
        "secretRef":"env:AIT_SERVER_CREDENTIAL_TEST"}}),
        )
        .unwrap();
    (state, channel)
}

fn input(channel: ChannelId, peer: &str, message: &str) -> Incoming {
    Incoming {
        channel_id: channel,
        peer: peer.into(),
        sender: "alert-bot".into(),
        thread: Some("origin-thread".into()),
        message_id: message.into(),
        text: message.into(),
        reply_to: Some(message.into()),
        passive: false,
        sender_type: Some("bot".into()),
    }
}

fn admit(state: &mut State, channel: ChannelId, peer: &str, message: &str) -> BotTurnId {
    let accepted = state.accept(input(channel, peer, message)).unwrap();
    serde_json::from_value(accepted["turn"]["id"].clone()).unwrap()
}

fn complete(state: &mut State, run: &crate::execution::Run) {
    state
        .progress(
            run.turn.id,
            TurnStatus::Completed,
            Some("done".into()),
            None,
        )
        .unwrap();
    state.finished(run.turn.conversation_id);
}

#[test]
fn restart_rotates_shared_context_and_resumes_only_unsent_inputs_in_order() {
    for status in [
        TurnStatus::Sending,
        TurnStatus::Running,
        TurnStatus::NeedsApproval,
    ] {
        let fixture = Fixture::new();
        let (mut state, channel) = active(&fixture);
        let interrupted = admit(&mut state, channel, "group-a", "interrupted");
        let first = state.ready().unwrap().pop().unwrap();
        state.progress(interrupted, status, None, None).unwrap();
        let queued = admit(&mut state, channel, "group-a", "queued");
        let other = admit(&mut state, channel, "group-b", "other");
        let context_id = state.catalog.contexts[&first.conversation.bot_id];
        let old_context = state.conversations[&context_id].clone();
        drop(state);

        let mut restarted = fixture.state();
        let context = restarted.conversations[&context_id].clone();
        assert_ne!(context.session_id, old_context.session_id);
        assert_eq!(context.generation, old_context.generation + 1);
        assert_eq!(
            restarted.turn(interrupted).unwrap().status,
            TurnStatus::Uncertain
        );
        assert_eq!(
            restarted.turn(interrupted).unwrap().generation,
            first.turn.generation
        );
        assert_eq!(
            restarted
                .accept(input(channel, "group-a", "interrupted"))
                .unwrap()["duplicate"],
            true
        );
        let run = restarted.ready().unwrap().pop().unwrap();
        assert_eq!(run.turn.id, queued);
        assert_eq!(run.turn.generation, first.turn.generation + 1);
        assert_eq!(run.conversation.session_id, context.session_id);
        complete(&mut restarted, &run);
        let run = restarted.ready().unwrap().pop().unwrap();
        assert_eq!(run.turn.id, other);
        assert_eq!(run.conversation.peer, "group-b");
        assert_eq!(run.conversation.thread.as_deref(), Some("origin-thread"));
        assert_eq!(run.conversation.session_id, context.session_id);
        complete(&mut restarted, &run);
        drop(restarted);

        let mut restarted = fixture.state();
        assert_eq!(
            restarted.conversations[&context_id].session_id,
            context.session_id
        );
        assert!(restarted.ready().unwrap().is_empty());
        assert_eq!(
            restarted.turn(interrupted).unwrap().status,
            TurnStatus::Uncertain
        );
    }
}

#[test]
fn a_restart_between_context_and_source_writes_never_reuses_the_interrupted_session() {
    let fixture = Fixture::new();
    let (mut state, channel) = active(&fixture);
    let interrupted = admit(&mut state, channel, "group-a", "running");
    let run = state.ready().unwrap().pop().unwrap();
    let queued = admit(&mut state, channel, "group-a", "queued");
    let mut context = state.conversations[&run.owner.conversation_id].clone();
    state.renew(&mut context).unwrap();
    drop(state);

    let mut restarted = fixture.state();
    let next = restarted.ready().unwrap().pop().unwrap();
    assert_eq!(next.turn.id, queued);
    assert_ne!(next.conversation.session_id, run.conversation.session_id);
    assert_ne!(next.conversation.session_id, context.session_id);
    assert_eq!(next.turn.generation, run.turn.generation + 1);
    assert_eq!(
        restarted.turn(interrupted).unwrap().status,
        TurnStatus::Uncertain
    );
}

#[test]
fn recovery_mends_queued_receipts_if_a_previous_start_stopped_after_renewal() {
    let fixture = Fixture::new();
    let (mut state, _) = active(&fixture);
    let request = WorkspaceRequest {
        workspace_id: WORKSPACE.into(),
    };
    let opened = state.open(&request).unwrap();
    let conversation: ConversationId =
        serde_json::from_value(opened["conversation"]["id"].clone()).unwrap();
    for message in ["running", "queued"] {
        state
            .execute(
                "bot.message.send.request",
                json!({"workspaceId":WORKSPACE,
            "conversationId":conversation,"messageId":message,"text":message}),
            )
            .unwrap();
    }
    let run = state.ready().unwrap().pop().unwrap();
    state
        .progress(run.turn.id, TurnStatus::Uncertain, None, None)
        .unwrap();
    let mut context = state.conversations[&conversation].clone();
    // A crash after the context write, before the queued turn's generation is updated.
    state.renew(&mut context).unwrap();
    drop(state);

    let mut restarted = fixture.state();
    assert_eq!(
        restarted.conversations[&conversation].session_id,
        context.session_id
    );
    let next = restarted.ready().unwrap().pop().unwrap();
    assert_eq!(next.turn.message_id, "queued");
    assert_eq!(next.turn.generation, context.generation);
    assert_eq!(next.conversation.session_id, context.session_id);
    assert_eq!(
        restarted.turn(run.turn.id).unwrap().status,
        TurnStatus::Uncertain
    );
}

#[test]
fn recovering_one_bot_preserves_another_bots_context() {
    let fixture = Fixture::new();
    let (mut state, channel) = active(&fixture);
    state
        .execute("bot.setup.request", setup(OTHER_WORKSPACE))
        .unwrap();
    let other = state.current(OTHER_WORKSPACE).unwrap().id;
    let other_context = state.context(other).unwrap();
    admit(&mut state, channel, "group-a", "running");
    let first = state.ready().unwrap().pop().unwrap();
    drop(state);

    let restarted = fixture.state();
    assert_ne!(
        restarted.conversations[&first.owner.conversation_id].session_id,
        first.conversation.session_id
    );
    assert_eq!(restarted.conversations[&other_context.id], other_context);
}

#[test]
fn recovery_rotates_a_paused_bot_without_starting_work_until_resume() {
    let fixture = Fixture::new();
    let (mut state, channel) = active(&fixture);
    admit(&mut state, channel, "group-a", "running");
    let interrupted = state.ready().unwrap().pop().unwrap();
    state
        .execute("bot.pause.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();
    drop(state);

    let mut restarted = fixture.state();
    assert!(restarted.ready().unwrap().is_empty());
    assert_eq!(
        restarted.turn(interrupted.turn.id).unwrap().status,
        TurnStatus::Uncertain
    );
    restarted
        .execute("bot.resume.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();
    let next = admit(&mut restarted, channel, "group-b", "new-alert");
    let run = restarted.ready().unwrap().pop().unwrap();
    assert_eq!(run.turn.id, next);
    assert_ne!(
        run.conversation.session_id,
        interrupted.conversation.session_id
    );
}

#[test]
fn interrupted_delivery_is_not_retried_and_does_not_rotate_a_healthy_context() {
    let fixture = Fixture::new();
    let (mut state, channel) = active(&fixture);
    let first = admit(&mut state, channel, "group-a", "first");
    let run = state.ready().unwrap().pop().unwrap();
    complete(&mut state, &run);
    state.delivery(first, DeliveryStatus::Sending).unwrap();
    let pending = admit(&mut state, channel, "group-b", "second");
    let next = state.ready().unwrap().pop().unwrap();
    complete(&mut state, &next);
    drop(state);

    let mut restarted = fixture.state();
    assert_eq!(
        restarted.turn(first).unwrap().delivery,
        DeliveryStatus::Uncertain
    );
    assert_eq!(restarted.turn(first).unwrap().status, TurnStatus::Completed);
    assert_eq!(
        restarted.pending_delivery(channel).unwrap().unwrap().0.id,
        pending
    );
    admit(&mut restarted, channel, "group-c", "third");
    let next = restarted.ready().unwrap().pop().unwrap();
    assert_eq!(next.conversation.session_id, run.conversation.session_id);
}

#[test]
fn legacy_routes_without_a_shared_context_index_recover_without_replay() {
    let fixture = Fixture::new();
    let (mut state, channel) = active(&fixture);
    let interrupted = admit(&mut state, channel, "group-a", "legacy");
    state
        .progress(interrupted, TurnStatus::Uncertain, None, None)
        .unwrap();
    let queued = admit(&mut state, channel, "group-a", "queued");
    assert!(state.catalog.contexts.is_empty());
    drop(state);

    let mut restarted = fixture.state();
    let next = restarted.ready().unwrap().pop().unwrap();
    assert_eq!(next.turn.id, queued);
    assert_eq!(
        restarted.turn(interrupted).unwrap().status,
        TurnStatus::Uncertain
    );
    assert_eq!(next.turn.generation, 1);
}
