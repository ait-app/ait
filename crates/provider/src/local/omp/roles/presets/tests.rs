use super::*;

#[test]
fn names_and_raw_entries_follow_native_bounds() {
    for name in ["Work", "team-2", "deep_think"] {
        assert!(valid_name(name));
    }
    for name in ["", "../secret", "a\nvalue", "2fast", "a b"] {
        assert!(!valid_name(name));
    }
    assert!(!valid_name(&"a".repeat(129)));
    assert!(
        parse_roles(&json!({"modelRoles":{"default":"@slow","slow":"test/model:high"}})).is_ok()
    );
    for raw in [
        json!({}),
        json!({"modelRoles":true}),
        json!({"modelRoles":{"default":""}}),
    ] {
        assert!(parse_roles(&raw).is_err());
    }
    assert!(valid_thinking("auto"));
    assert!(!valid_thinking("arbitrary"));
}

#[test]
fn expansion_consumes_the_request_and_preserves_unrelated_fields() {
    let preset = StoredAgentConfig {
        model: Some("chosen".to_owned()),
        thinking_option_id: Some("high".to_owned()),
        feature_values: Some(BTreeMap::from([
            ("modelRole.default".to_owned(), json!("test/model")),
            ("modelPreset".to_owned(), Value::Null),
        ])),
        ..Default::default()
    };
    let mut presets = Presets {
        entries: BTreeMap::from([(
            "work".to_owned(),
            Entry {
                raw: json!({"modelRoles":{"default":"test/model"}}),
                source: "global",
                configuration: Ok(preset.clone()),
            },
        )]),
        basis: Value::Null,
    };
    let config = StoredAgentConfig {
        system_prompt: Some("keep".to_owned()),
        feature_values: Some(BTreeMap::from([
            ("autoRetry".to_owned(), json!(false)),
            ("modelRole.removed".to_owned(), json!("old/model")),
            ("modelPreset".to_owned(), json!("work")),
        ])),
        ..Default::default()
    };
    let resolved = presets.resolve(&config).unwrap();
    assert_eq!(resolved.model, preset.model);
    assert_eq!(resolved.system_prompt, config.system_prompt);
    let features = resolved.feature_values.as_ref().unwrap();
    assert_eq!(features["modelRole.removed"], "unset");
    assert_eq!(features["autoRetry"], false);
    assert!(features["modelPreset"].is_null());
    assert_eq!(presets.resolve(&resolved).unwrap(), resolved);
    assert!(presets.has_role("default"));
    assert!(presets.has_assignment("default", "test/model"));
    presets
        .entries
        .insert("custom".to_owned(), presets.entries["work"].clone());
    let mut named_custom = config;
    named_custom
        .feature_values
        .as_mut()
        .unwrap()
        .insert("modelPreset".to_owned(), json!("custom"));
    assert_eq!(presets.resolve(&named_custom).unwrap().model, preset.model);
}
