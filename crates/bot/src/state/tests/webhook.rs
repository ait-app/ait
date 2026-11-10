use domain::bot::{DeliveryStatus, TurnStatus};
use domain::{BotTurnId, ChannelId};
use model::ErrorCode;
use model::bot::Store;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::state::State;
use crate::test_support::{Fixture, OTHER_WORKSPACE, WORKSPACE, setup};

fn channel(state: &mut State, workspace: &str, enabled: bool) -> Result<ChannelId, ErrorCode> {
    let id = ChannelId::generate();
    state.create_webhook_channel(crate::protocol::decode(json!({
        "workspaceId":workspace,"channelId":id,
        "config":{"kind":"webhook","endpointId":id.to_string(),"url":"https://center.example/hook"},"enabled":enabled,
    }))?)?;
    Ok(id)
}

fn send(
    state: &mut State,
    channel: ChannelId,
    thread: &str,
    message: &str,
) -> Result<Value, ErrorCode> {
    state.execute(
        "bot.webhook.message.send.request",
        json!({"workspaceId":WORKSPACE,"channelId":channel,"peer":"webhook:endpoint","sender":"hub",
            "thread":thread,"messageId":message,"text":"fix the build"}),
    )
}

fn turn_id(receipt: &Value) -> BotTurnId {
    serde_json::from_value(receipt["turn"]["id"].clone()).unwrap()
}

#[test]
fn webhook_channels_are_published_only_while_enabled() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    let channel = channel(&mut state, WORKSPACE, false).unwrap();
    assert_eq!(
        state.execute("bot.channel.create.request", json!({"workspaceId":WORKSPACE,"channelId":ChannelId::generate(),"config":{"kind":"hub"},"enabled":true})),
        Err(ErrorCode::InvalidMessage)
    );
    assert!(
        state.webhook_bots().is_empty(),
        "a disabled Webhook channel is not reachable"
    );
    assert!(
        state.channels().iter().all(|c| c.id != channel),
        "no transport runs for a Webhook channel until enabled"
    );
    state
        .execute(
            "bot.channel.update.request",
            json!({"workspaceId":WORKSPACE,"channelId":channel,"enabled":true}),
        )
        .unwrap();
    let bots = state.webhook_bots();
    assert_eq!(bots.len(), 1);
    assert_eq!(
        (
            bots[0].workspace_id.as_str(),
            bots[0].channel_id,
            bots[0].title.as_str(),
            bots[0].state
        ),
        (WORKSPACE, channel, "workspace", "active")
    );
    state
        .execute("bot.pause.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();
    assert_eq!(state.webhook_bots()[0].state, "paused");
}

/// A Workspace Bot with an enabled Webhook channel, and its built-in channel.
fn webhook_bot() -> (Fixture, State, ChannelId, ChannelId) {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let bot = state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    let builtin: ChannelId =
        serde_json::from_value(bot["bot"]["builtinChannelId"].clone()).unwrap();
    let channel = channel(&mut state, WORKSPACE, true).unwrap();
    (fixture, state, channel, builtin)
}

#[test]
fn the_center_reaches_only_webhook_turns() {
    let (_fixture, mut state, channel, builtin) = webhook_bot();
    // Input lands in one conversation per center thread, and retries are absorbed.
    let first = send(&mut state, channel, "c1", "automation:1").unwrap();
    assert_eq!(
        (first["accepted"].clone(), first["duplicate"].clone()),
        (json!(true), json!(false))
    );
    assert_eq!(
        send(&mut state, channel, "c1", "automation:1").unwrap()["duplicate"],
        true
    );
    let second = send(&mut state, channel, "c2", "automation:2").unwrap();
    assert_ne!(
        first["turn"]["conversationId"],
        second["turn"]["conversationId"]
    );
    assert_eq!(
        send(&mut state, builtin, "c1", "automation:3"),
        Err(ErrorCode::Forbidden)
    );

    let turn = turn_id(&first);
    state.webhook_turn(WORKSPACE, turn).unwrap();
    assert_eq!(
        state.webhook_turn(OTHER_WORKSPACE, turn),
        Err(ErrorCode::Forbidden)
    );
    let opened = state
        .execute(
            "bot.conversation.open.request",
            json!({"workspaceId":WORKSPACE}),
        )
        .unwrap();
    let owner = state
        .execute(
            "bot.message.send.request",
            json!({"workspaceId":WORKSPACE,"conversationId":opened["conversation"]["id"],"messageId":"m","text":"hi"}),
        )
        .unwrap();
    assert_eq!(
        state.webhook_turn(WORKSPACE, turn_id(&owner)),
        Err(ErrorCode::Forbidden),
        "the built-in conversation is the owner's"
    );
    assert_eq!(
        state.execute(
            "bot.webhook.conversation.close.request",
            json!({"workspaceId":WORKSPACE,"conversationId":opened["conversation"]["id"]})
        ),
        Err(ErrorCode::Forbidden)
    );
}

#[test]
fn a_completed_reply_waits_for_one_acknowledgement() {
    let (_fixture, mut state, channel, _) = webhook_bot();
    let turn = turn_id(&send(&mut state, channel, "c1", "automation:1").unwrap());
    let ack = |status: &str| json!({"workspaceId":WORKSPACE,"turnId":turn,"status":status});
    assert_eq!(
        state.execute("bot.webhook.delivery.ack.request", ack("delivered")),
        Err(ErrorCode::BotConflict),
        "nothing to deliver yet"
    );
    state
        .progress(turn, TurnStatus::Completed, Some("Fixed.".into()), None)
        .unwrap();
    assert_eq!(state.turn(turn).unwrap().delivery, DeliveryStatus::Pending);
    assert!(state.pending_delivery(channel).unwrap().is_some());
    state
        .execute("bot.webhook.delivery.ack.request", ack("delivered"))
        .unwrap();
    state
        .execute("bot.webhook.delivery.ack.request", ack("uncertain"))
        .unwrap();
    assert_eq!(
        state.turn(turn).unwrap().delivery,
        DeliveryStatus::Delivered
    );
    assert!(state.pending_delivery(channel).unwrap().is_none());
}

#[test]
fn closing_forgets_a_webhook_conversation_for_good() {
    let (fixture, mut state, channel, _) = webhook_bot();
    let kept = turn_id(&send(&mut state, channel, "c1", "automation:1").unwrap());
    let closed = send(&mut state, channel, "c2", "automation:2").unwrap();
    let close = json!({"workspaceId":WORKSPACE,"conversationId":closed["turn"]["conversationId"]});
    let busy: domain::ConversationId =
        serde_json::from_value(closed["turn"]["conversationId"].clone()).unwrap();
    state.running.insert(busy, CancellationToken::new());
    assert_eq!(
        state.execute("bot.webhook.conversation.close.request", close.clone()),
        Err(ErrorCode::BotConflict)
    );
    state.running.remove(&busy);
    state
        .execute("bot.webhook.conversation.close.request", close.clone())
        .unwrap();
    state
        .execute("bot.webhook.conversation.close.request", close)
        .unwrap();
    assert_eq!(
        state.webhook_turn(WORKSPACE, turn_id(&closed)),
        Err(ErrorCode::Forbidden)
    );
    assert!(
        state
            .ready()
            .unwrap()
            .iter()
            .all(|run| run.turn.id != turn_id(&closed)),
        "its queued input never runs"
    );
    assert!(state.session_actions.iter().any(|(_, action)| matches!(
        action,
        provider::service::bot_execution::Action::Close { .. }
    )));
    // A restart does not bring the closed conversation back.
    assert_eq!(fixture.store.conversations().unwrap().len(), 1);
    let restarted = fixture.state();
    assert_eq!(restarted.webhook_turn(WORKSPACE, kept), Ok(()));
    assert_eq!(
        restarted.webhook_turn(WORKSPACE, turn_id(&closed)),
        Err(ErrorCode::Forbidden)
    );
}

fn webhook(state: &mut State, endpoint: &str, url: &str) -> Result<ChannelId, ErrorCode> {
    let id = ChannelId::generate();
    state.create_webhook_channel(crate::protocol::decode(json!({
        "workspaceId":WORKSPACE,"channelId":id,
        "config":{"kind":"webhook","endpointId":endpoint,"url":url},"enabled":true,
    }))?)?;
    Ok(id)
}

#[test]
fn only_the_center_creates_webhook_channels() {
    let (_fixture, mut state, _, _) = webhook_bot();
    assert_eq!(
        state.execute(
            "bot.channel.create.request",
            json!({"workspaceId":WORKSPACE,"channelId":ChannelId::generate(),"enabled":true,
                "config":{"kind":"webhook","endpointId":"e1","url":"https://aitapp.cn/hook"}}),
        ),
        Err(ErrorCode::InvalidMessage)
    );
    for url in ["http://aitapp.cn/hook", "ftp://x", "not a url"] {
        assert_eq!(
            webhook(&mut state, "e1", url),
            Err(ErrorCode::InvalidMessage),
            "{url}"
        );
    }
    webhook(&mut state, "e1", "http://127.0.0.1:8080/hook").unwrap();
    assert_eq!(
        webhook(&mut state, "e1", "https://aitapp.cn/other"),
        Err(ErrorCode::BotConflict),
        "one endpoint backs one channel"
    );
}

#[test]
fn webhook_channels_are_published_and_reached_by_channel() {
    let (_fixture, mut state, channel, builtin) = webhook_bot();
    let first = webhook(&mut state, "e1", "https://aitapp.cn/api/v1/hook/e1").unwrap();
    let second = webhook(&mut state, "e2", "https://aitapp.cn/api/v1/hook/e2").unwrap();
    let published: Vec<_> = state
        .webhook_bots()
        .into_iter()
        .map(|entry| (entry.channel_id, entry.kind))
        .collect();
    assert_eq!(
        published,
        [
            (channel, "webhook"),
            (first, "webhook"),
            (second, "webhook")
        ]
    );
    let receipt = send(&mut state, first, "c1", "automation:1").unwrap();
    state.webhook_turn(WORKSPACE, turn_id(&receipt)).unwrap();
    assert_eq!(
        send(&mut state, builtin, "c1", "automation:2"),
        Err(ErrorCode::Forbidden)
    );

    // The endpoint is the center's: only the toggle changes.
    assert_eq!(
        state.execute(
            "bot.channel.update.request",
            json!({"workspaceId":WORKSPACE,"channelId":first,"enabled":true,
                "config":{"kind":"webhook","endpointId":"e1","url":"https://evil.example/"}}),
        ),
        Err(ErrorCode::BotConflict)
    );
    state
        .execute(
            "bot.channel.update.request",
            json!({"workspaceId":WORKSPACE,"channelId":first,"enabled":false}),
        )
        .unwrap();
    assert_eq!(
        send(&mut state, first, "c1", "automation:3"),
        Err(ErrorCode::Forbidden),
        "a disabled webhook channel takes no input"
    );
    assert_eq!(
        state.webhook_turn(WORKSPACE, turn_id(&receipt)),
        Err(ErrorCode::Forbidden)
    );
    state.check_webhook_channel(WORKSPACE, first).unwrap();
    assert_eq!(
        state.check_webhook_channel(WORKSPACE, builtin),
        Err(ErrorCode::BotConflict)
    );

    state
        .execute(
            "bot.channel.delete.request",
            json!({"workspaceId":WORKSPACE,"channelId":first}),
        )
        .unwrap();
    assert_eq!(state.released_webhooks, [first]);
    state
        .execute("bot.remove.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();
    assert_eq!(
        state.released_webhooks,
        [first, channel, second],
        "retiring the Bot releases its remaining endpoints once"
    );
}
