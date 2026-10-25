use super::*;
use std::collections::BTreeMap;

#[test]
fn legacy_protocol_values_migrate_without_rejecting_native_feature_overrides() {
    for selection in [None, Some(json!("rpc")), Some(json!("acp"))] {
        let mut values = BTreeMap::from([("fastMode".to_owned(), json!(true))]);
        if let Some(selection) = selection {
            values.insert("transport".to_owned(), selection);
        }
        let config = StoredAgentConfig {
            mode_id: Some("ask".to_owned()),
            feature_values: Some(values),
            ..StoredAgentConfig::default()
        };
        assert_eq!(validate(&config), Ok(()));
    }
    for selection in [json!("unknown"), json!(true)] {
        let config = StoredAgentConfig {
            feature_values: Some(BTreeMap::from([("transport".to_owned(), selection)])),
            ..StoredAgentConfig::default()
        };
        assert_eq!(validate(&config), Err(AgentSessionError::Rejected));
    }
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
fn commands_validate_native_facts() {
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

#[test]
fn acp_model_catalog_preserves_the_rpc_limit_without_expanding_other_adapters() {
    let option = json!({"id":"model","type":"select","currentValue":"0","options":(0..8192).map(|id| json!({"value":id.to_string(),"name":"Model"})).collect::<Vec<_>>()});
    let snapshot = json!({"configOptions":[option]});
    assert!(crate::local::acp_config::state(&snapshot).is_err());
    assert_eq!(
        crate::local::acp_config::state_with_limit(&snapshot, 8192).unwrap()[0]["options"]
            .as_array()
            .unwrap()
            .len(),
        8192
    );
    let mut too_many = snapshot;
    too_many["configOptions"][0]["options"]
        .as_array_mut()
        .unwrap()
        .push(json!({"value":"overflow","name":"Overflow"}));
    assert!(crate::local::acp_config::state_with_limit(&too_many, 8192).is_err());
}
