use super::*;

#[test]
fn validates_opaque_model_routes_and_rejects_unsupported_controls() {
    assert_eq!(
        model_route(r#"["provider","model/variant"]"#).unwrap(),
        ["provider", "model/variant"]
    );
    for id in [
        "model",
        r#"["one"]"#,
        r#"["","model"]"#,
        r#"["p","m","extra"]"#,
    ] {
        assert!(model_route(id).is_err());
    }
    assert!(validate(&StoredAgentConfig::default()).is_ok());
    assert!(
        validate(&StoredAgentConfig {
            mode_id: Some("invalid".to_owned()),
            ..StoredAgentConfig::default()
        })
        .is_err()
    );
    assert!(
        validate(&StoredAgentConfig {
            feature_values: Some(BTreeMap::from([("unsupported".to_owned(), json!(true))])),
            ..StoredAgentConfig::default()
        })
        .is_err()
    );
    assert!(
        validate(&StoredAgentConfig {
            thinking_option_id: Some("invalid\nlevel".to_owned()),
            ..StoredAgentConfig::default()
        })
        .is_err()
    );
    assert!(
        validate(&StoredAgentConfig {
            system_prompt: Some("bad\0prompt".to_owned()),
            ..StoredAgentConfig::default()
        })
        .is_err()
    );
}

#[test]
fn persisted_legacy_features_are_accepted_and_ignored() {
    let config = StoredAgentConfig {
        feature_values: Some(BTreeMap::from([
            ("autoCompaction".to_owned(), json!(false)),
            ("autoRetry".to_owned(), json!(false)),
            ("slowMode".to_owned(), json!(true)),
            ("transport".to_owned(), json!("acp")),
        ])),
        ..StoredAgentConfig::default()
    };

    assert!(validate(&config).is_ok());
    assert_eq!(requested_fast_mode(&config), None);
    let state = json!({"fastModeEnabled":false,"fastModeActive":false});
    let features = features(requested_fast_mode(&config), Some(&state));
    assert_eq!(features.len(), 1);
    assert_eq!(features[0]["id"], "fastMode");
    for (key, value) in [("slowMode", json!("on")), ("transport", json!("ws"))] {
        assert!(
            validate(&StoredAgentConfig {
                feature_values: Some(BTreeMap::from([(key.to_owned(), value)])),
                ..StoredAgentConfig::default()
            })
            .is_err()
        );
    }
}

#[test]
fn runtime_validates_native_facts() {
    let state = json!({"sessionId":"id","model":{"provider":"p","id":"m"},"thinkingLevel":"high"});
    assert_eq!(
        runtime(&state).unwrap().model,
        Some(r#"["p","m"]"#.to_owned())
    );
    assert_eq!(state_model(&state).unwrap().as_deref(), Some("p/m"));
    assert_eq!(state_model(&json!({"model":null})).unwrap(), None);
    assert!(runtime(&json!({})).is_err());
}

#[test]
fn launch_arguments_follow_paseo_order_and_default_to_full_access() {
    let config = StoredAgentConfig {
        system_prompt: Some("  Be brief.  ".to_owned()),
        ..StoredAgentConfig::default()
    };
    let launch = Launch {
        model: Some(model_argument(r#"["provider","model/variant"]"#).unwrap()),
        thinking: thinking_level("high"),
        session: Some("/sessions/file.jsonl"),
        ..Launch::default()
    };

    assert_eq!(
        launch_arguments(&config, &launch),
        [
            "--mode",
            "rpc-ui",
            "--approval-mode",
            "yolo",
            "--model",
            "provider/model/variant",
            "--thinking",
            "high",
            "--session",
            "/sessions/file.jsonl",
            "--append-system-prompt",
            "Be brief.",
        ]
    );
    let probe = Launch {
        ephemeral: true,
        ..Launch::default()
    };
    let ask_cached = StoredAgentConfig {
        mode_id: Some("ask-cached".to_owned()),
        system_prompt: Some(" ".to_owned()),
        ..StoredAgentConfig::default()
    };
    assert_eq!(
        launch_arguments(&ask_cached, &probe),
        [
            "--mode",
            "rpc-ui",
            "--approval-mode",
            "always-ask-cached",
            "--no-session"
        ]
    );
    for (mode, native) in [
        (None, "yolo"),
        (Some("full"), "yolo"),
        (Some("write"), "write"),
        (Some("ask"), "always-ask"),
    ] {
        assert_eq!(approval_mode(mode), native);
    }
}

#[test]
fn only_known_thinking_levels_reach_the_launch() {
    for level in [
        "auto", "off", "minimal", "low", "medium", "high", "xhigh", "max",
    ] {
        assert_eq!(thinking_level(level), Some(level));
    }
    assert!(
        validate(&StoredAgentConfig {
            thinking_option_id: Some("vendor-defined-effort".to_owned()),
            ..StoredAgentConfig::default()
        })
        .is_ok()
    );
    assert_eq!(thinking_level("vendor-defined-effort"), None);
}

#[test]
fn thinking_options_follow_paseo_for_each_kind_of_model() {
    let ids = |options: &[Value]| -> Vec<String> {
        options
            .iter()
            .map(|option| option["id"].as_str().unwrap().to_owned())
            .collect()
    };
    let default_of = |options: &[Value]| {
        options
            .iter()
            .filter(|option| option["isDefault"] == true)
            .map(|option| option["id"].clone())
            .collect::<Vec<_>>()
    };

    for model in [
        json!({}),
        json!({"reasoning":false,"thinking":{"efforts":["max"]}}),
    ] {
        assert_eq!(thinking_options(&model), None);
    }
    for model in [
        json!({"reasoning":true}),
        json!({"reasoning":true,"thinking":{"efforts":[]}}),
        json!({"reasoning":true,"thinking":{"efforts":["vendor-defined-effort",null]}}),
    ] {
        let (options, default) = thinking_options(&model).unwrap();
        assert_eq!(
            ids(&options),
            [
                "auto", "off", "minimal", "low", "medium", "high", "xhigh", "max"
            ]
        );
        assert_eq!(default, "medium");
        assert_eq!(default_of(&options), [json!("medium")]);
    }
    let (options, default) = thinking_options(&json!({"reasoning":true,"thinking":{
        "efforts":["max","low","auto","vendor-defined-effort"],"defaultLevel":"max"}}))
    .unwrap();
    assert_eq!(ids(&options), ["auto", "low", "max"]);
    assert_eq!(default, "max");
    assert_eq!(options[0]["label"], "Auto");
    assert_eq!(default_of(&options), [json!("max")]);
    let (_, default) =
        thinking_options(&json!({"reasoning":true,"thinking":{"efforts":["high","low"]}})).unwrap();
    assert_eq!(default, "low");
    // A reported default the model does not list falls back to its first effort.
    let (_, default) = thinking_options(
        &json!({"reasoning":true,"thinking":{"efforts":["high"],"defaultLevel":"low"}}),
    )
    .unwrap();
    assert_eq!(default, "high");
}

#[test]
fn model_labels_are_the_normalized_last_segment_with_the_route_described() {
    for (name, label) in [
        ("Fixture model", "Fixture model"),
        ("meta/llama_3  70b", "llama 3 70b"),
        ("Vendor: Plain_model", "Plain model"),
        ("trailing/", "trailing"),
    ] {
        assert_eq!(model_label(name), label);
    }
    let entry = model_entry(
        &json!({"provider":"p","id":"m/v","name":"Vendor: Large_model","reasoning":true}),
        &json!({"provider":"p","id":"m/v"}),
    )
    .unwrap();
    assert_eq!(entry["id"], r#"["p","m/v"]"#);
    assert_eq!(entry["label"], "Large model");
    assert_eq!(entry["description"], "p/m/v");
    assert_eq!(entry["isDefault"], true);
    assert_eq!(entry["defaultThinkingOptionId"], "medium");
    assert_eq!(entry["metadata"], json!({"provider":"p","modelId":"m/v"}));
    // As in Paseo, a non-reasoning model has no thinking options at all.
    let unnamed = model_entry(&json!({"provider":"p","id":"bare"}), &Value::Null).unwrap();
    assert_eq!(unnamed["label"], "bare");
    assert!(unnamed.get("thinkingOptions").is_none());
    assert!(unnamed.get("defaultThinkingOptionId").is_none());
}

#[test]
fn fast_is_offered_only_when_omp_reports_both_facts() {
    let requested = StoredAgentConfig {
        feature_values: Some(BTreeMap::from([("fastMode".to_owned(), json!(true))])),
        ..StoredAgentConfig::default()
    };
    for state in [
        json!({}),
        json!({"fastModeEnabled":true}),
        json!({"fastModeEnabled":true,"fastModeActive":"yes"}),
    ] {
        assert!(features(Some(true), Some(&state)).is_empty());
    }
    assert!(features(Some(true), None).is_empty());

    let inactive = json!({"fastModeEnabled":true,"fastModeActive":false});
    let toggle = &features(None, Some(&inactive))[0];
    assert_eq!(toggle["value"], true);
    assert_eq!(toggle["label"], "Fast");
    assert_eq!(toggle["description"], FAST_INACTIVE);
    let active = json!({"fastModeEnabled":false,"fastModeActive":false});
    let toggle = &features(requested_fast_mode(&requested), Some(&active))[0];
    assert_eq!(toggle["value"], true);
    assert!(toggle.get("description").is_none());
}

#[test]
fn provider_options_follow_paseos_strict_schema() {
    let with = |options: Value| StoredAgentConfig {
        provider_options: Some(serde_json::from_value(options).unwrap()),
        ..StoredAgentConfig::default()
    };
    let accepted = with(
        json!({"smolModel":"p/small","slowModel":"p/deep","planModel":"p/plan",
        "rpcTimeoutMs":1500}),
    );
    assert!(validate(&accepted).is_ok());
    assert!(validate(&with(json!({}))).is_ok());
    for options in [
        json!({"sessionDir":"/sessions"}),
        json!({"fast_mode":true}),
        json!({"unknown":1}),
        json!({"smolModel":""}),
        json!({"smolModel":"  "}),
        json!({"slowModel":"p/m\n"}),
        json!({"planModel":1}),
        json!({"smolModel":"x".repeat(1025)}),
        json!({"rpcTimeoutMs":0}),
        json!({"rpcTimeoutMs":-5}),
        json!({"rpcTimeoutMs":1.5}),
        json!({"rpcTimeoutMs":"100"}),
        json!({"rpcTimeoutMs":2_147_483_648_u64}),
    ] {
        assert!(validate(&with(options.clone())).is_err(), "{options}");
    }

    // Model roles follow the approval mode, in Paseo's order.
    assert_eq!(
        launch_arguments(&accepted, &Launch::default()),
        [
            "--mode",
            "rpc-ui",
            "--approval-mode",
            "yolo",
            "--smol",
            "p/small",
            "--slow",
            "p/deep",
            "--plan",
            "p/plan",
        ]
    );
    let defaults = Timeouts::default();
    assert_eq!(
        timeouts(&accepted, defaults),
        Timeouts {
            ready: Duration::from_millis(1500),
            request: Duration::from_millis(1500),
        }
    );
    assert_eq!(timeouts(&StoredAgentConfig::default(), defaults), defaults);
}
