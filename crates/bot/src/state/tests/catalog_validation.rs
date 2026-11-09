use domain::BotId;
use domain::bot::BotState;

use super::*;

fn push_bot(catalog: &mut domain::bot::Catalog, state: BotState) {
    let mut bot = catalog.bots[0].clone();
    bot.id = BotId::generate();
    bot.builtin_channel_id = ChannelId::generate();
    bot.state = state;
    let mut channel = catalog.channels[0].clone();
    channel.id = bot.builtin_channel_id;
    channel.bot_id = bot.id;
    catalog.bots.push(bot);
    catalog.channels.push(channel);
}

#[test]
fn catalog_rejects_duplicate_owners_missing_builtin_channels_and_invalid_configuration() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    let original = state.catalog.clone();
    for case in [
        "duplicate_bot",
        "missing_builtin",
        "duplicate_channel",
        "deleted_builtin",
        "invalid_provider",
        "invalid_workspace",
        "second_live_bot",
    ] {
        let mut catalog = original.clone();
        match case {
            "duplicate_bot" => catalog.bots.push(catalog.bots[0].clone()),
            "missing_builtin" => catalog.channels.clear(),
            "duplicate_channel" => catalog.channels.push(catalog.channels[0].clone()),
            "deleted_builtin" => catalog.channels[0].deleted = true,
            "invalid_provider" => catalog.bots[0].execution.provider.clear(),
            "invalid_workspace" => catalog.bots[0].workspace_id.clear(),
            "second_live_bot" => push_bot(&mut catalog, BotState::Active),
            _ => unreachable!("fixed fixture case"),
        }
        assert_eq!(
            crate::state::catalog::validate(&catalog),
            Err(ErrorCode::InvalidMessage),
            "{case}"
        );
    }
    let mut retired = original.clone();
    push_bot(
        &mut retired,
        BotState::Retired {
            at: "2026-01-01T00:00:00Z".into(),
        },
    );
    assert_eq!(crate::state::catalog::validate(&retired), Ok(()));
    let mut oversized = original;
    oversized.bots = vec![oversized.bots[0].clone(); 129];
    assert_eq!(
        crate::state::catalog::validate(&oversized),
        Err(ErrorCode::ResourceExhausted)
    );
}

#[test]
fn capacity_counts_only_live_bots_and_channels() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    let template = state.catalog.clone();
    let mut catalog = domain::bot::Catalog::default();
    for index in 0..200 {
        let mut bot = template.bots[0].clone();
        bot.id = BotId::generate();
        bot.workspace_id = format!("wks_{index:016x}");
        bot.builtin_channel_id = ChannelId::generate();
        if index >= 128 {
            bot.state = BotState::Retired {
                at: "2026-01-01T00:00:00Z".into(),
            };
        }
        let mut channel = template.channels[0].clone();
        channel.id = bot.builtin_channel_id;
        channel.bot_id = bot.id;
        catalog.bots.push(bot);
        catalog.channels.push(channel);
    }
    let owner = catalog.bots[0].id;
    for _ in 0..300 {
        catalog.channels.push(domain::bot::Channel {
            id: ChannelId::generate(),
            bot_id: owner,
            enabled: false,
            cursor: None,
            sync_token: None,
            deleted: true,
            config: ChannelConfig::Telegram {
                token_ref: "env:AIT_SERVER_CREDENTIAL_TEST".into(),
                allowed_senders: vec!["1".into()],
                allow_groups: false,
            },
        });
    }
    assert_eq!(crate::state::catalog::validate(&catalog), Ok(()));
    catalog.bots[128].state = BotState::Active;
    assert_eq!(
        crate::state::catalog::validate(&catalog),
        Err(ErrorCode::ResourceExhausted)
    );
}

#[test]
fn setup_that_cannot_save_the_catalog_keeps_memory() {
    let fixture = Fixture::new();
    fixture.store.save_memory(WORKSPACE, "prefs").unwrap();
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(OTHER_WORKSPACE))
        .unwrap();
    let template = state.catalog.clone();
    for index in 1..128 {
        let mut bot = template.bots[0].clone();
        bot.id = BotId::generate();
        bot.workspace_id = format!("wks_{index:016x}");
        bot.builtin_channel_id = ChannelId::generate();
        let mut channel = template.channels[0].clone();
        channel.id = bot.builtin_channel_id;
        channel.bot_id = bot.id;
        state.catalog.bots.push(bot);
        state.catalog.channels.push(channel);
    }
    let mut cleared = setup(WORKSPACE);
    cleared["clearMemory"] = json!(true);

    assert_eq!(
        state.execute("bot.setup.request", cleared),
        Err(ErrorCode::ResourceExhausted)
    );
    assert_eq!(fixture.store.memory(WORKSPACE).unwrap(), "prefs");
}

#[test]
fn bot_configuration_updates_apply_to_new_sessions_and_retired_bots_reject_edits() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    let updated = state
        .execute(
            "bot.update.request",
            json!({"workspaceId":WORKSPACE,"execution":{"provider":"other-fixture"}}),
        )
        .unwrap();
    assert_eq!(updated["bot"]["execution"]["provider"], "other-fixture");
    assert_eq!(updated["configurationAppliesToNewSessions"], true);
    assert_eq!(
        state.execute(
            "bot.update.request",
            json!({"workspaceId":WORKSPACE,"name":"renamed","execution":{"provider":"test"}})
        ),
        Err(ErrorCode::InvalidMessage)
    );
    state.retire(&[WORKSPACE.into()]).unwrap();
    assert_eq!(
        state.execute(
            "bot.update.request",
            json!({"workspaceId":WORKSPACE,"execution":{"provider":"test"}})
        ),
        Err(ErrorCode::BotNotFound)
    );
}

#[test]
fn channel_creation_is_idempotent_and_account_identity_cannot_change_in_place() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    let id = ChannelId::generate();
    let request = json!({"workspaceId":WORKSPACE,"channelId":id,"enabled":true,
        "config":{"kind":"feishu","appId":"app-1","secretRef":"env:AIT_SERVER_CREDENTIAL_TEST",
            "allowedSenders":["sender"]}});
    assert_eq!(
        state
            .execute("bot.channel.create.request", request.clone())
            .unwrap()["created"],
        true
    );
    assert_eq!(
        state
            .execute("bot.channel.create.request", request.clone())
            .unwrap()["created"],
        false
    );
    let mut changed = request.clone();
    changed["enabled"] = json!(false);
    assert_eq!(
        state.execute("bot.channel.create.request", changed),
        Err(ErrorCode::IdempotencyConflict)
    );
    for config in [
        json!({"kind":"telegram","tokenRef":"env:AIT_SERVER_CREDENTIAL_TEST","allowedSenders":["sender"]}),
        json!({"kind":"feishu","appId":"app-2","secretRef":"env:AIT_SERVER_CREDENTIAL_TEST","allowedSenders":["sender"]}),
    ] {
        assert_eq!(
            state.execute(
                "bot.channel.update.request",
                json!({"workspaceId":WORKSPACE,"channelId":id,"enabled":true,"config":config})
            ),
            Err(ErrorCode::BotConflict)
        );
    }
    let mut config = request["config"].clone();
    config["allowedSenders"] = json!(["new-sender"]);
    let updated = state
        .execute(
            "bot.channel.update.request",
            json!({"workspaceId":WORKSPACE,"channelId":id,"enabled":true,"config":config}),
        )
        .unwrap();
    assert_eq!(
        updated["channel"]["config"]["allowedSenders"],
        json!(["new-sender"])
    );
}

#[test]
fn stored_credential_references_may_name_only_their_own_channel() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    let id = ChannelId::generate();
    let other = ChannelId::generate();
    let config = |reference: String| {
        json!({"workspaceId":WORKSPACE,"channelId":id,"config":{"kind":"feishu","appId":"cli_a",
            "secretRef":reference,"allowedSenders":["ou_a"]}})
    };

    assert_eq!(
        state.execute(
            "bot.channel.create.request",
            config(crate::ports::stored_credential_reference(other))
        ),
        Err(ErrorCode::InvalidMessage)
    );
    assert!(
        state
            .execute(
                "bot.channel.create.request",
                config(crate::ports::stored_credential_reference(id))
            )
            .is_ok()
    );
}

#[test]
fn reply_references_may_exceed_the_identifier_limit_but_stay_bounded() {
    use crate::channels::Incoming;

    let fixture = Fixture::new();
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    let channel = state
        .execute(
            "bot.channel.create.request",
            json!({"workspaceId":WORKSPACE,"channelId":ChannelId::generate(),"enabled":true,
                "config":{"kind":"telegram","tokenRef":"env:AIT_SERVER_CREDENTIAL_TEST",
                    "allowedSenders":["1"]}}),
        )
        .unwrap();
    let channel_id = fixture.channel(&channel["channel"]).id;
    let incoming = |message: &str, reply_to: String| Incoming {
        channel_id,
        peer: "peer".into(),
        sender: "1".into(),
        thread: None,
        message_id: message.into(),
        text: "hello".into(),
        reply_to: Some(reply_to),
    };
    assert!(state.accept(incoming("1", "t".repeat(4096))).is_ok());
    assert_eq!(
        state.accept(incoming("2", "t".repeat(4097))).err(),
        Some(ErrorCode::InvalidMessage)
    );
    assert_eq!(
        state.accept(incoming("3", String::new())).err(),
        Some(ErrorCode::InvalidMessage)
    );
}
