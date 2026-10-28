use super::*;

#[test]
fn a_later_deferred_model_switch_supersedes_an_earlier_thinking_command() {
    let config = StoredAgentConfig {
        thinking_option_id: Some("high".into()),
        ..StoredAgentConfig::default()
    };
    let thinking = ConfigPatch {
        thinking_option_id: NullableSetting::Set("high".into()),
        ..ConfigPatch::default()
    };
    let pending = DeferredConfig::merge(None, &StoredAgentConfig::default(), &config, &thinking);
    let model = ConfigPatch {
        model_id: NullableSetting::Set("fixture/other".into()),
        ..ConfigPatch::default()
    };
    let next = DeferredConfig::merge(Some(&pending), &config, &model.apply(&config), &model);
    assert_eq!(next.config.thinking_option_id, None);
    assert_eq!(
        next.previous_config.as_ref(),
        Some(&StoredAgentConfig::default())
    );
    assert_eq!(next.patch().thinking_option_id, NullableSetting::Unchanged);
    assert_eq!(next.patch().model_id, model.model_id);
}

#[test]
fn absent_turn_preference_is_not_an_explicit_medium_reset() {
    let requested = StoredAgentConfig::default();
    let previous = StoredAgentConfig {
        thinking_option_id: Some("high".into()),
        ..StoredAgentConfig::default()
    };
    let state = json!({});
    let turn = Plan::new(&requested, &previous, &state, Intent::Turn).unwrap();
    assert_eq!(turn.thinking, None);
    let cleared = ConfigPatch {
        thinking_option_id: NullableSetting::Clear,
        ..ConfigPatch::default()
    };
    let explicit_clear = Plan::new(&requested, &previous, &state, Intent::Patch(&cleared)).unwrap();
    assert_eq!(explicit_clear.thinking, Some("medium"));
}

#[test]
fn malformed_live_thinking_cannot_replace_valid_state() {
    let mut state = json!({"thinkingLevel":"high"});
    for level in [
        json!(""),
        json!("x".repeat(513)),
        json!("bad\nlevel"),
        json!(42),
    ] {
        assert!(!observe_thinking(
            &mut state,
            &json!({"type":"thinking_level_changed","level":level}),
        ));
        assert_eq!(state["thinkingLevel"], "high");
        assert_eq!(
            Selection::read(&json!({"thinkingLevel":level})).thinking,
            None
        );
    }
    assert!(observe_thinking(
        &mut state,
        &json!({"type":"thinking_level_changed","level":"future"}),
    ));
    assert_eq!(Selection::read(&state).thinking.as_deref(), Some("future"));
}

#[test]
fn older_deferred_configuration_defaults_missing_command_flags() {
    let pending: DeferredConfig = serde_json::from_value(json!({"config": {}})).unwrap();
    assert_eq!(pending.patch().model_id, NullableSetting::Unchanged);
    assert_eq!(
        pending.patch().thinking_option_id,
        NullableSetting::Unchanged
    );
    assert!(!pending.clears_thinking());
}
