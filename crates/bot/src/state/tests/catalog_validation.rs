use super::*;

#[test]
fn catalog_rejects_duplicate_owners_missing_builtin_channels_and_invalid_configuration() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let bot = BotId::generate();
    state.execute("bot.create.request", creation(bot)).unwrap();
    let original = state.catalog.clone();
    for case in [
        "duplicate_bot",
        "missing_builtin",
        "duplicate_channel",
        "deleted_builtin",
        "invalid_name",
        "invalid_workspace",
    ] {
        let mut catalog = original.clone();
        match case {
            "duplicate_bot" => catalog.bots.push(catalog.bots[0].clone()),
            "missing_builtin" => catalog.channels.clear(),
            "duplicate_channel" => catalog.channels.push(catalog.channels[0].clone()),
            "deleted_builtin" => catalog.channels[0].deleted = true,
            "invalid_name" => catalog.bots[0].name.clear(),
            "invalid_workspace" => catalog.bots[0].workspace_id.clear(),
            _ => unreachable!("fixed fixture case"),
        }
        assert_eq!(
            crate::state::catalog::validate(&catalog),
            Err(ErrorCode::InvalidMessage),
            "{case}"
        );
    }
    let mut oversized = original;
    oversized.bots = vec![oversized.bots[0].clone(); 129];
    assert_eq!(
        crate::state::catalog::validate(&oversized),
        Err(ErrorCode::ResourceExhausted)
    );
}

#[test]
fn bot_configuration_updates_apply_to_new_sessions_and_archived_bots_reject_edits() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let bot = BotId::generate();
    state.execute("bot.create.request", creation(bot)).unwrap();
    let updated = state
        .execute(
            "bot.update.request",
            json!({"botId":bot,
        "instructions":"new instructions","execution":{"provider":"other-fixture"}}),
        )
        .unwrap();
    assert_eq!(updated["bot"]["instructions"], "new instructions");
    assert_eq!(updated["bot"]["execution"]["provider"], "other-fixture");
    assert_eq!(updated["configurationAppliesToNewSessions"], true);
    state
        .execute("bot.archive.request", json!({"botId":bot}))
        .unwrap();
    assert_eq!(
        state.execute("bot.update.request", json!({"botId":bot,"name":"renamed"})),
        Err(ErrorCode::BotConflict)
    );
}

#[test]
fn channel_creation_is_idempotent_and_account_identity_cannot_change_in_place() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let bot = BotId::generate();
    state.execute("bot.create.request", creation(bot)).unwrap();
    let id = ChannelId::generate();
    let request = json!({"botId":bot,"channelId":id,"enabled":true,
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
                json!({"botId":bot,"channelId":id,"enabled":true,"config":config})
            ),
            Err(ErrorCode::BotConflict)
        );
    }
    let mut config = request["config"].clone();
    config["allowedSenders"] = json!(["new-sender"]);
    let updated = state
        .execute(
            "bot.channel.update.request",
            json!({"botId":bot,"channelId":id,"enabled":true,"config":config}),
        )
        .unwrap();
    assert_eq!(
        updated["channel"]["config"]["allowedSenders"],
        json!(["new-sender"])
    );
}
