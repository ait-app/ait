use super::*;
use std::collections::BTreeMap;

#[test]
fn protocols_validate_native_controls_without_implicitly_switching_existing_sessions() {
    assert_eq!(is_acp(&StoredAgentConfig::default()), Ok(false));
    for (selection, valid) in [
        (json!("rpc"), true),
        (json!("acp"), true),
        (json!("unknown"), false),
        (json!(true), false),
    ] {
        let mut config = StoredAgentConfig {
            feature_values: Some(BTreeMap::from([("transport".to_owned(), selection)])),
            ..StoredAgentConfig::default()
        };
        assert_eq!(validate(&config).is_ok(), valid);
        if valid {
            config.mode_id = Some("plan".to_owned());
            assert_eq!(validate(&config).is_ok(), is_acp(&config).unwrap());
        }
    }
    let config = StoredAgentConfig {
        feature_values: Some(BTreeMap::from([
            ("transport".to_owned(), json!("acp")),
            ("fastMode".to_owned(), json!(true)),
        ])),
        ..StoredAgentConfig::default()
    };
    assert_eq!(validate(&config), Err(AgentSessionError::Rejected));
}

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
            thinking_option_id: Some("invalid".to_owned()),
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
fn runtime_and_commands_validate_native_facts() {
    let state = json!({"sessionId":"id","model":{"provider":"p","id":"m"},"thinkingLevel":"high"});
    assert_eq!(
        runtime(&state).unwrap().model,
        Some(r#"["p","m"]"#.to_owned())
    );
    assert!(runtime(&json!({})).is_err());
    assert_eq!(
        commands(&json!({"commands":[{"name":"review","source":"skill"}]})).unwrap()[0]["name"],
        "review"
    );
    assert!(commands(&json!({"commands":[{"name":""}]})).is_err());
    assert_eq!(
        features(&StoredAgentConfig::default(), ServiceTiers::default()).len(),
        2
    );
}

#[test]
fn model_metadata_does_not_invent_or_accept_invalid_thinking_levels() {
    for model in [
        json!({}),
        json!({"reasoning":false,"thinking":{"efforts":["max"]}}),
        json!({"reasoning":true}),
    ] {
        assert_eq!(
            model_thinking_options(&model).unwrap(),
            json!([{"id":"off","label":"off"}])
                .as_array()
                .unwrap()
                .clone()
        );
    }
    for efforts in [
        json!(["off"]),
        json!(["max", "max"]),
        json!([null]),
        json!("high"),
    ] {
        assert!(
            model_thinking_options(&json!({"reasoning":true,"thinking":{"efforts":efforts}}))
                .is_err()
        );
    }
}

#[test]
fn tier_toggles_require_each_native_capability_and_preserve_selected_values() {
    let config = StoredAgentConfig {
        feature_values: Some(BTreeMap::from([
            ("autoRetry".to_owned(), json!(false)),
            ("fastMode".to_owned(), json!(true)),
            ("slowMode".to_owned(), json!(true)),
        ])),
        ..StoredAgentConfig::default()
    };
    for tiers in [
        ServiceTiers {
            fast: true,
            slow: false,
        },
        ServiceTiers {
            fast: false,
            slow: true,
        },
    ] {
        let toggles = features(&config, tiers);
        assert_eq!(toggles.len(), 3);
        assert_eq!(toggles[1]["value"], false);
        assert_eq!(
            toggles[2]["id"],
            if tiers.fast { "fastMode" } else { "slowMode" }
        );
        assert_eq!(toggles[2]["value"], true);
    }
}
