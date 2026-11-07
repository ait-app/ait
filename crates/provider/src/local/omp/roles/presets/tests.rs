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

#[test]
fn an_empty_preset_directory_exposes_no_preset_control() {
    let presets = Presets::default();
    let mut features = Vec::new();
    presets.append_features(&StoredAgentConfig::default(), &mut features);
    assert!(features.is_empty(), "{features:?}");
    // A saved snapshot of a preset that no longer exists must not resurrect the empty control,
    // while the roles it names stay selectable.
    let config = StoredAgentConfig {
        feature_values: Some(BTreeMap::from([(
            "modelPresetRoles".to_owned(),
            json!(r#"{"default":"test/model"}"#),
        )])),
        ..Default::default()
    };
    let mut features = Vec::new();
    presets.append_features(&config, &mut features);
    assert!(
        !features
            .iter()
            .any(|feature| feature["id"] == "modelPreset"),
        "{features:?}"
    );
    assert!(
        features
            .iter()
            .any(|feature| feature["id"] == "modelRole.default")
    );
}

#[test]
fn named_presets_render_their_whole_option_list() {
    let preset = StoredAgentConfig {
        model: Some("test/model".to_owned()),
        thinking_option_id: Some("high".to_owned()),
        ..Default::default()
    };
    let presets = Presets {
        entries: BTreeMap::from([
            (
                "work".to_owned(),
                Entry {
                    raw: json!({"modelRoles":{"default":"test/model"}}),
                    source: "global",
                    configuration: Ok(preset),
                },
            ),
            (
                "broken".to_owned(),
                Entry {
                    raw: json!({"modelRoles":false}),
                    source: "project",
                    configuration: Err("Invalid preset roles".to_owned()),
                },
            ),
        ]),
        basis: Value::Null,
    };
    let mut features = Vec::new();
    presets.append_features(&StoredAgentConfig::default(), &mut features);
    let control = features
        .iter()
        .find(|feature| feature["id"] == "modelPreset")
        .unwrap();
    assert_eq!(control["value"], "__custom__");
    let options = control["options"].as_array().unwrap();
    assert_eq!(options.len(), 3);
    assert_eq!(options[0]["id"], "__custom__");
    let work = options
        .iter()
        .find(|option| option["id"] == "work")
        .expect("the valid preset must be offered");
    assert_eq!(work["metadata"]["configuration"]["modelId"], "test/model");
    assert_eq!(
        work["metadata"]["configuration"]["thinkingOptionId"],
        "high"
    );
    let broken = options
        .iter()
        .find(|option| option["id"] == "broken")
        .expect("a malformed preset stays visible");
    assert_eq!(broken["metadata"]["error"], "Invalid preset roles");
}

#[test]
fn a_settings_probe_falls_back_to_a_directory_omp_accepts() {
    use super::editor::probe_cwd;

    // A provider sheet has no project, so the caller's cwd may be empty or unusable.
    assert!(probe_cwd("").starts_with('/'));
    assert!(probe_cwd("/definitely/not/a/directory").starts_with('/'));
    // A real project directory is preserved so project-scoped presets keep their own scope.
    assert_eq!(probe_cwd("/tmp"), "/tmp");
    // The daemon's own directory is the filesystem root under a launcher, where OMP refuses to
    // start; it must not be used as the probe directory.
    assert_ne!(probe_cwd("/"), "/");
}
