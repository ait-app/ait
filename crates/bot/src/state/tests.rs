use domain::bot::{
    ChannelConfig, GithubReviewContext, GithubReviewTrigger, TurnContext, TurnStatus,
};
use domain::{BotTurnId, ChannelId, ConversationId};
use model::ErrorCode;
use model::bot::Store;
use serde_json::json;

use crate::channels::Incoming;
use crate::test_support::{Fixture, OTHER_WORKSPACE, WORKSPACE, setup};

mod catalog_validation;
mod lifecycle;
mod permissions;
mod webhook;

#[test]
fn unconfigured_workspace_has_no_bot_and_no_session_actions() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let got = state
        .execute("bot.get.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();
    assert_eq!(got, json!({"bot":null}));
    assert!(state.session_actions.is_empty());
    assert!(fixture.store.catalog().unwrap().bots.is_empty());
}

#[test]
fn setup_is_idempotent_and_single_per_workspace() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let first = state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    assert_eq!(first["created"], true);
    assert_eq!(first["bot"]["state"], json!({"kind":"active"}));
    assert!(first["bot"].get("id").is_none());
    let again = state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    assert_eq!(again["created"], false);
    let mut other = setup(WORKSPACE);
    other["execution"]["provider"] = json!("other");
    assert_eq!(
        state.execute("bot.setup.request", other),
        Err(ErrorCode::BotConflict)
    );
    assert_eq!(fixture.store.catalog().unwrap().bots.len(), 1);
}

#[test]
fn removed_v1_methods_are_unknown() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    for method in [
        "bot.create.request",
        "bot.list.request",
        "bot.archive.request",
        "bot.permission.resolve.request",
    ] {
        assert_eq!(
            state.execute(method, json!({})),
            Err(ErrorCode::MethodNotFound)
        );
    }
}

#[test]
fn setup_clears_memory_only_when_requested_for_a_new_bot() {
    let fixture = Fixture::new();
    fixture.store.save_memory(WORKSPACE, "prefs").unwrap();
    let mut state = fixture.state();
    let mut cleared = setup(WORKSPACE);
    cleared["clearMemory"] = json!(true);
    let mut invalid = cleared.clone();
    invalid["execution"]["provider"] = json!("");

    assert_eq!(
        state.execute("bot.setup.request", invalid),
        Err(ErrorCode::InvalidMessage)
    );
    assert_eq!(fixture.store.memory(WORKSPACE).unwrap(), "prefs");
    state.execute("bot.setup.request", cleared.clone()).unwrap();
    assert_eq!(fixture.store.memory(WORKSPACE).unwrap(), "");
    fixture.store.save_memory(WORKSPACE, "kept").unwrap();
    assert_eq!(
        state.execute("bot.setup.request", cleared).unwrap()["created"],
        false
    );
    assert_eq!(fixture.store.memory(WORKSPACE).unwrap(), "kept");
}

#[test]
fn channel_capabilities_status_and_conversations_are_scoped_to_the_bot() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let created = state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    let channel = created["bot"]["builtinChannelId"].clone();
    let opened = state
        .execute(
            "bot.conversation.open.request",
            json!({"workspaceId":WORKSPACE}),
        )
        .unwrap();

    assert_eq!(
        state
            .execute("bot.channel.types.request", json!({}))
            .unwrap(),
        json!({"supported":["builtin","telegram","feishu","github"],"email":{"supported":false,"planned":true}})
    );
    let status = state
        .execute(
            "bot.channel.status.request",
            json!({"workspaceId":WORKSPACE,"channelId":channel}),
        )
        .unwrap();
    assert_eq!(status["enabled"], true);
    assert_eq!(status["connection"], serde_json::Value::Null);
    let conversations = state
        .execute(
            "bot.conversation.list.request",
            json!({"workspaceId":WORKSPACE}),
        )
        .unwrap();
    assert_eq!(conversations["conversations"].as_array().unwrap().len(), 1);
    assert_eq!(
        conversations["conversations"][0]["id"],
        opened["conversation"]["id"]
    );
    assert!(!conversations.to_string().contains("sessionId"));
    assert!(!conversations.to_string().contains("botId"));
    assert!(!opened.to_string().contains("botId"));
    assert_eq!(
        state.execute(
            "bot.conversation.list.request",
            json!({"workspaceId":OTHER_WORKSPACE})
        ),
        Err(ErrorCode::BotNotFound)
    );
}

fn external(channel: ChannelId, sender: &str, message: &str) -> Incoming {
    Incoming {
        channel_id: channel,
        peer: "group".into(),
        sender: sender.into(),
        thread: Some("thread".into()),
        message_id: message.into(),
        text: "hello".into(),
        reply_to: None,
        context: None,
    }
}

#[test]
fn github_review_triggers_for_the_same_head_reuse_the_first_turn() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    let channel = ChannelId::generate();
    state
        .execute(
            "bot.channel.create.request",
            json!({
                "workspaceId":WORKSPACE,
                "channelId":channel,
                "enabled":true,
                "config":{
                    "kind":"github",
                    "appId":"123",
                    "privateKeyRef":"env:AIT_SERVER_CREDENTIAL_GITHUB_PRIVATE_KEY",
                    "webhookSecretRef":"env:AIT_SERVER_CREDENTIAL_GITHUB_WEBHOOK_SECRET",
                    "installationId":"456",
                    "mention":"ait-review",
                    "repositories":[{
                        "fullName":"owner/repository",
                        "projectId":"project-1",
                        "allowedSenders":["maintainer"]
                    }]
                }
            }),
        )
        .unwrap();
    let context = |trigger, workspace: &str| {
        Some(TurnContext::GithubReview(GithubReviewContext {
            repository: "owner/repository".to_owned(),
            project_id: "project-1".to_owned(),
            pull_number: 7,
            title: "Change".to_owned(),
            url: "https://github.com/owner/repository/pull/7".to_owned(),
            base_ref: "main".to_owned(),
            head_ref: "feature".to_owned(),
            base_sha: "1".repeat(40),
            head_sha: "2".repeat(40),
            trigger,
            trigger_user: "maintainer".to_owned(),
            trigger_comment_id: None,
            workspace_id: workspace.to_owned(),
        }))
    };
    let request = |text: &str, context| Incoming {
        channel_id: channel,
        peer: "repo:5:pr:7".to_owned(),
        sender: "github-review".to_owned(),
        thread: Some("2".repeat(40)),
        message_id: "github-review:stable".to_owned(),
        text: text.to_owned(),
        reply_to: None,
        context,
    };
    let first = state
        .accept(request(
            "mention mandate",
            context(GithubReviewTrigger::Mention, "workspace-first"),
        ))
        .unwrap();
    let duplicate = state
        .accept(request(
            "assignment mandate",
            context(GithubReviewTrigger::ReviewerAssignment, "workspace-second"),
        ))
        .unwrap();
    assert_eq!(duplicate["duplicate"], true);
    assert_eq!(duplicate["turn"]["id"], first["turn"]["id"]);
}

#[test]
fn catalog_crud_requires_exclusive_ownership_and_retains_deleted_history() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let created = state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    state
        .execute("bot.setup.request", setup(OTHER_WORKSPACE))
        .unwrap();
    let builtin = created["bot"]["builtinChannelId"].clone();
    assert_eq!(
        state.execute(
            "bot.channel.delete.request",
            json!({"workspaceId":WORKSPACE,"channelId":builtin})
        ),
        Err(ErrorCode::BotConflict)
    );
    let channel = ChannelId::generate();
    let config = json!({"kind":"telegram","tokenRef":"env:AIT_SERVER_CREDENTIAL_TELEGRAM","allowedSenders":["alice","bob"]});
    let created_channel = state
        .execute(
            "bot.channel.create.request",
            json!({"workspaceId":WORKSPACE,"channelId":channel,"config":config,"enabled":true}),
        )
        .unwrap();
    assert!(!created_channel.to_string().contains("botId"));
    assert_eq!(created_channel["channel"]["id"], json!(channel));
    let listed = state
        .execute("bot.channel.list.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();
    assert!(!listed.to_string().contains("botId"));
    let updated = state
        .execute(
            "bot.channel.update.request",
            json!({"workspaceId":WORKSPACE,"channelId":channel,"enabled":true}),
        )
        .unwrap();
    assert!(!updated.to_string().contains("botId"));
    assert_eq!(
        state.execute(
            "bot.channel.create.request",
            json!({"workspaceId":OTHER_WORKSPACE,"channelId":ChannelId::generate(),"config":config})
        ),
        Err(ErrorCode::BotConflict)
    );
    assert_eq!(
        state.execute(
            "bot.channel.update.request",
            json!({"workspaceId":OTHER_WORKSPACE,"channelId":channel,"enabled":false})
        ),
        Err(ErrorCode::BotNotFound)
    );
    assert_channel_history(&fixture, &mut state, channel);
}

#[test]
fn interrupted_turns_block_replay_until_explicit_reset_and_history_is_paginated() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    let conversation: ConversationId = serde_json::from_value(
        state
            .execute(
                "bot.conversation.open.request",
                json!({"workspaceId":WORKSPACE}),
            )
            .unwrap()["conversation"]["id"]
            .clone(),
    )
    .unwrap();
    let receipt = state
        .execute(
            "bot.message.send.request",
            json!({"workspaceId":WORKSPACE,"conversationId":conversation,"messageId":"one","text":"first"}),
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
            json!({"workspaceId":WORKSPACE,"conversationId":conversation}),
        )
        .unwrap();
    restarted
        .execute(
            "bot.message.send.request",
            json!({"workspaceId":WORKSPACE,"conversationId":conversation,"messageId":"two","text":"second"}),
        )
        .unwrap();
    let ready = restarted.ready().unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].turn.generation, 1);
    let history = restarted
        .execute(
            "bot.history.get.request",
            json!({"workspaceId":WORKSPACE,"conversationId":conversation,"limit":1}),
        )
        .unwrap();
    assert_eq!(history["turns"][0]["text"], "first");
    assert_eq!(restarted.execute("bot.history.get.request", json!({"workspaceId":WORKSPACE,"conversationId":conversation,"afterSequence":history["nextSequence"],"limit":1})).unwrap()["turns"][0]["text"], "second");
    assert_eq!(
        restarted.execute(
            "bot.history.get.request",
            json!({"workspaceId":WORKSPACE,"conversationId":conversation,"limit":51})
        ),
        Err(ErrorCode::InvalidMessage)
    );
}

#[test]
fn unvalidated_credentials_and_missing_workspaces_fail_before_persistence() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    assert_eq!(
        state.execute("bot.setup.request", setup("missing")),
        Err(ErrorCode::WorkspaceNotFound)
    );
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    for reference in [
        "raw-secret",
        "env:HOME",
        "env:AIT_SERVER_CREDENTIAL_bad",
        "env:AIT_SERVER_CREDENTIAL_",
    ] {
        assert_eq!(state.execute("bot.channel.create.request", json!({"workspaceId":WORKSPACE,"channelId":ChannelId::generate(),"config":{"kind":"telegram","tokenRef":reference,"allowedSenders":["1"]}})), Err(ErrorCode::InvalidMessage));
    }
    assert_eq!(
        state.execute("unknown", json!({})),
        Err(ErrorCode::MethodNotFound)
    );
}

fn assert_channel_history(fixture: &Fixture, state: &mut super::State, channel: ChannelId) {
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
            json!({"workspaceId":WORKSPACE,"channelId":channel,"enabled":false}),
        )
        .unwrap();
    state
        .execute(
            "bot.channel.delete.request",
            json!({"workspaceId":WORKSPACE,"channelId":channel}),
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
            .execute("bot.channel.list.request", json!({"workspaceId":WORKSPACE}))
            .unwrap()["channels"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let mut reloaded = fixture.state();
    for workspace in [WORKSPACE, OTHER_WORKSPACE] {
        assert_eq!(
            reloaded
                .execute("bot.get.request", json!({"workspaceId":workspace}))
                .unwrap()["bot"]["workspaceId"],
            workspace
        );
    }
    assert!(matches!(
        fixture.store.catalog().unwrap().channels[0].config,
        ChannelConfig::Builtin
    ));
}
