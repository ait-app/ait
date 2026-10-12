use domain::bot::{ChannelConfig, TurnStatus};
use domain::{BotId, BotTurnId, ChannelId, ConversationId};
use model::ErrorCode;
use model::bot::Store;
use serde_json::json;

use crate::channels::Incoming;
use crate::test_support::{Fixture, creation};

mod lifecycle;

fn external(channel: ChannelId, sender: &str, message: &str) -> Incoming {
    Incoming {
        channel_id: channel,
        peer: "group".into(),
        sender: sender.into(),
        thread: Some("thread".into()),
        message_id: message.into(),
        text: "hello".into(),
        reply_to: None,
    }
}

#[test]
fn catalog_crud_requires_exclusive_ownership_and_retains_deleted_history() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let bot = BotId::generate();
    let other = BotId::generate();
    let created = state.execute("bot.create.request", creation(bot)).unwrap();
    state
        .execute("bot.create.request", creation(other))
        .unwrap();
    assert_eq!(
        state.execute("bot.create.request", creation(bot)).unwrap()["created"],
        false
    );
    let mut changed = creation(bot);
    changed["name"] = json!("different");
    assert_eq!(
        state.execute("bot.create.request", changed),
        Err(ErrorCode::IdempotencyConflict)
    );
    let builtin = created["bot"]["builtinChannelId"].clone();
    assert_eq!(
        state.execute(
            "bot.channel.delete.request",
            json!({"botId":bot,"channelId":builtin})
        ),
        Err(ErrorCode::BotConflict)
    );
    let channel = ChannelId::generate();
    let config = json!({"kind":"telegram","tokenRef":"env:AIT_SERVER_CREDENTIAL_TELEGRAM","allowedSenders":["alice","bob"]});
    state
        .execute(
            "bot.channel.create.request",
            json!({"botId":bot,"channelId":channel,"config":config,"enabled":true}),
        )
        .unwrap();
    assert_eq!(
        state.execute(
            "bot.channel.create.request",
            json!({"botId":other,"channelId":ChannelId::generate(),"config":config})
        ),
        Err(ErrorCode::BotConflict)
    );
    assert_eq!(
        state.execute(
            "bot.channel.update.request",
            json!({"botId":other,"channelId":channel,"enabled":false})
        ),
        Err(ErrorCode::BotNotFound)
    );
    assert_channel_history(&fixture, &mut state, bot, channel);
}

#[test]
fn interrupted_turns_block_replay_until_explicit_reset_and_history_is_paginated() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let bot = BotId::generate();
    state.execute("bot.create.request", creation(bot)).unwrap();
    let conversation: ConversationId = serde_json::from_value(
        state
            .execute("bot.conversation.open.request", json!({"botId":bot}))
            .unwrap()["conversation"]["id"]
            .clone(),
    )
    .unwrap();
    let receipt = state
        .execute(
            "bot.message.send.request",
            json!({"botId":bot,"conversationId":conversation,"messageId":"one","text":"first"}),
        )
        .unwrap();
    let turn: BotTurnId = serde_json::from_value(receipt["turn"]["id"].clone()).unwrap();
    assert_eq!(state.ready().unwrap().len(), 1);
    let mut restarted = fixture.state();
    assert_eq!(restarted.turn(turn).unwrap().status, TurnStatus::Uncertain);
    assert!(restarted.ready().unwrap().is_empty());
    restarted
        .execute(
            "bot.conversation.reset.request",
            json!({"botId":bot,"conversationId":conversation}),
        )
        .unwrap();
    restarted
        .execute(
            "bot.message.send.request",
            json!({"botId":bot,"conversationId":conversation,"messageId":"two","text":"second"}),
        )
        .unwrap();
    let ready = restarted.ready().unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].turn.generation, 1);
    let history = restarted
        .execute(
            "bot.history.get.request",
            json!({"botId":bot,"conversationId":conversation,"limit":1}),
        )
        .unwrap();
    assert_eq!(history["turns"][0]["text"], "first");
    assert_eq!(restarted.execute("bot.history.get.request", json!({"botId":bot,"conversationId":conversation,"afterSequence":history["nextSequence"],"limit":1})).unwrap()["turns"][0]["text"], "second");
    assert_eq!(
        restarted.execute(
            "bot.history.get.request",
            json!({"botId":bot,"conversationId":conversation,"limit":51})
        ),
        Err(ErrorCode::InvalidMessage)
    );
    restarted
        .execute(
            "bot.update.request",
            json!({"botId":bot,"name":"renamed","enabled":false}),
        )
        .unwrap();
    restarted
        .execute("bot.archive.request", json!({"botId":bot}))
        .unwrap();
    assert_eq!(
        restarted
            .execute("bot.get.request", json!({"botId":bot}))
            .unwrap()["bot"]["name"],
        "renamed"
    );
}

#[test]
fn unvalidated_credentials_and_missing_workspaces_fail_before_persistence() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let bot = BotId::generate();
    let mut missing = creation(bot);
    missing["workspaceId"] = json!("missing");
    assert_eq!(
        state.execute("bot.create.request", missing),
        Err(ErrorCode::WorkspaceNotFound)
    );
    state.execute("bot.create.request", creation(bot)).unwrap();
    for reference in [
        "raw-secret",
        "env:HOME",
        "env:AIT_SERVER_CREDENTIAL_bad",
        "env:AIT_SERVER_CREDENTIAL_",
    ] {
        assert_eq!(state.execute("bot.channel.create.request", json!({"botId":bot,"channelId":ChannelId::generate(),"config":{"kind":"telegram","tokenRef":reference,"allowedSenders":["1"]}})), Err(ErrorCode::InvalidMessage));
    }
    assert_eq!(
        state.execute("unknown", json!({})),
        Err(ErrorCode::MethodNotFound)
    );
}

fn assert_channel_history(
    fixture: &Fixture,
    state: &mut super::State,
    bot: BotId,
    channel: ChannelId,
) {
    let alice = state.accept(external(channel, "alice", "a1")).unwrap();
    let bob = state.accept(external(channel, "bob", "b1")).unwrap();
    assert_ne!(
        alice["turn"]["conversationId"],
        bob["turn"]["conversationId"]
    );
    assert_eq!(
        state.accept(external(channel, "alice", "a1")).unwrap()["duplicate"],
        true
    );
    assert_eq!(
        state.accept(external(channel, "bob", "a1")),
        Err(ErrorCode::IdempotencyConflict)
    );
    state.advance_cursor(channel, 100).unwrap();
    state.advance_cursor(channel, 50).unwrap();
    state
        .execute(
            "bot.channel.update.request",
            json!({"botId":bot,"channelId":channel,"enabled":false}),
        )
        .unwrap();
    state
        .execute(
            "bot.channel.delete.request",
            json!({"botId":bot,"channelId":channel}),
        )
        .unwrap();
    assert_eq!(
        fixture
            .store
            .catalog()
            .unwrap()
            .channels
            .iter()
            .find(|entry| entry.id == channel)
            .unwrap()
            .cursor,
        Some(100)
    );
    assert_eq!(
        state
            .execute("bot.channel.list.request", json!({"botId":bot}))
            .unwrap()["channels"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let mut reloaded = fixture.state();
    assert_eq!(
        reloaded
            .execute(
                "bot.list.request",
                json!({"workspaceId":"wks_0123456789abcdef"})
            )
            .unwrap()["bots"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(matches!(
        fixture.store.catalog().unwrap().channels[0].config,
        ChannelConfig::Builtin
    ));
}
