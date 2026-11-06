use super::*;
use serde_json::Value;

fn permissions_request(permissions: &Value) -> Value {
    json!({"workspaceId":WORKSPACE,"permissions":permissions})
}

#[test]
fn setup_stores_default_permissions_and_normalizes_mode() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let mut request = setup(WORKSPACE);
    request["execution"]["provider"] = json!("codex");
    request["execution"]["modeId"] = json!("full-access");
    let created = state.execute("bot.setup.request", request.clone()).unwrap();
    assert_eq!(created["bot"]["execution"]["modeId"], Value::Null);
    assert_eq!(
        created["bot"]["permissions"]["groups"],
        json!(["observe", "agents", "stop", "schedules"])
    );
    assert!(created["bot"].get("workspaces").is_none());
    assert_eq!(
        state.execute("bot.setup.request", request).unwrap()["created"],
        false
    );
}

#[test]
fn setup_with_permissions_is_idempotent_only_for_equal_normalized_permissions() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let mut request = setup(WORKSPACE);
    request["permissions"] = json!({"groups":["scripts"],"modes":{"test":["auto"]}});
    let created = state.execute("bot.setup.request", request.clone()).unwrap();
    assert_eq!(
        created["bot"]["permissions"]["groups"],
        json!(["observe", "scripts"])
    );
    assert_eq!(
        state.execute("bot.setup.request", request).unwrap()["created"],
        false
    );
    assert_eq!(
        state.execute("bot.setup.request", setup(WORKSPACE)),
        Err(ErrorCode::BotConflict)
    );
    let mut invalid = setup(OTHER_WORKSPACE);
    invalid["permissions"] = json!({"groups":[],"modes":{"test":[]}});
    assert_eq!(
        state.execute("bot.setup.request", invalid),
        Err(ErrorCode::InvalidMessage)
    );
}

#[test]
fn update_normalizes_mode_to_null() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    let updated = state
        .execute(
            "bot.update.request",
            json!({"workspaceId":WORKSPACE,"execution":{"provider":"codex","model":null,"modeId":"auto","thinking":null}}),
        )
        .unwrap();
    assert_eq!(updated["bot"]["execution"]["modeId"], Value::Null);
}

#[test]
fn permissions_update_is_validated_persisted_and_refused_after_removal() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    let updated = state
        .execute(
            "bot.permissions.update.request",
            permissions_request(&json!({"groups":["terminals"],"modes":{"codex":["auto"]}})),
        )
        .unwrap();
    assert_eq!(updated["appliesToNextTurn"], true);
    assert_eq!(
        updated["bot"]["permissions"]["groups"],
        json!(["observe", "terminals"])
    );
    let mut reloaded = fixture.state();
    let got = reloaded
        .execute("bot.get.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();
    assert_eq!(got["bot"]["permissions"], updated["bot"]["permissions"]);

    assert_eq!(
        reloaded.execute(
            "bot.permissions.update.request",
            permissions_request(&json!({"groups":[],"modes":{"codex":[]}})),
        ),
        Err(ErrorCode::InvalidMessage)
    );
    reloaded
        .execute("bot.remove.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();
    assert_eq!(
        reloaded.execute(
            "bot.permissions.update.request",
            permissions_request(&json!({"groups":[],"modes":{}})),
        ),
        Err(ErrorCode::BotNotFound)
    );
}

#[test]
fn any_provider_can_back_the_bot() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    for (workspace, provider) in [(WORKSPACE, "minimax"), (OTHER_WORKSPACE, "opencode")] {
        let mut request = setup(workspace);
        request["execution"]["provider"] = json!(provider);
        let created = state.execute("bot.setup.request", request).unwrap();
        assert_eq!(created["created"], true);
    }

    let legacy = Fixture::new();
    let mut state = legacy.state();
    let mut request = setup(WORKSPACE);
    request["execution"]["provider"] = json!("minimax");
    state.execute("bot.setup.request", request).unwrap();
    let mut reloaded = legacy.state();
    let opened = reloaded
        .execute(
            "bot.conversation.open.request",
            json!({"workspaceId":WORKSPACE}),
        )
        .unwrap();
    reloaded
        .execute(
            "bot.message.send.request",
            json!({"workspaceId":WORKSPACE,"conversationId":opened["conversation"]["id"],
                "messageId":"m1","text":"hi"}),
        )
        .unwrap();
    assert_eq!(reloaded.ready().unwrap().len(), 1);
}

#[test]
fn recorded_worktree_workspaces_are_bounded_and_persisted() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    assert_eq!(
        state.record_workspace(WORKSPACE, "wks_created"),
        Err(ErrorCode::BotNotFound)
    );
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    for index in 0..64 {
        state
            .record_workspace(WORKSPACE, &format!("wks_created_{index}"))
            .unwrap();
    }
    state.record_workspace(WORKSPACE, "wks_created_0").unwrap();
    assert_eq!(
        state.record_workspace(WORKSPACE, "wks_created_64"),
        Err(ErrorCode::ResourceExhausted)
    );
    assert_eq!(
        fixture.store.catalog().unwrap().bots[0].workspaces.len(),
        64
    );
}

#[test]
fn legacy_mode_is_normalized_on_load_and_identical_setup_is_idempotent() {
    let fixture = Fixture::new();
    let mut request = setup(WORKSPACE);
    request["execution"]["provider"] = json!("codex");
    fixture
        .state()
        .execute("bot.setup.request", request.clone())
        .unwrap();
    let mut legacy = fixture.store.catalog().unwrap();
    legacy.bots[0].execution.mode_id = Some("full-access".into());
    fixture.store.save_catalog(&legacy).unwrap();

    let mut state = fixture.state();

    assert_eq!(state.catalog.bots[0].execution.mode_id, None);
    assert_eq!(
        state.execute("bot.setup.request", request).unwrap()["created"],
        false
    );
}
