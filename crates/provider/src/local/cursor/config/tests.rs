use super::*;

#[test]
fn rejects_unsupported_settings_and_invalid_specs() {
    for config in [
        StoredAgentConfig {
            system_prompt: Some("custom".into()),
            ..StoredAgentConfig::default()
        },
        StoredAgentConfig {
            model: Some(String::new()),
            ..StoredAgentConfig::default()
        },
        StoredAgentConfig {
            mode_id: Some("bad\n".into()),
            ..StoredAgentConfig::default()
        },
        StoredAgentConfig {
            thinking_option_id: Some("x".repeat(1025)),
            ..StoredAgentConfig::default()
        },
        StoredAgentConfig {
            tool_policy: Some(json!({"preapproved":[]})),
            ..StoredAgentConfig::default()
        },
        StoredAgentConfig {
            feature_values: Some(std::collections::BTreeMap::from([(
                "unsupported".into(),
                json!(true),
            )])),
            ..StoredAgentConfig::default()
        },
        StoredAgentConfig {
            mcp_servers: Some(std::collections::BTreeMap::from([(
                "test".into(),
                json!({"type":"http","url":"https://example.test"}),
            )])),
            ..StoredAgentConfig::default()
        },
    ] {
        assert!(validate(&config).is_err());
    }
    assert!(
        validate_spec(&AgentSessionSpec {
            provider: PROVIDER.into(),
            cwd: "relative".into(),
            config: StoredAgentConfig::default()
        })
        .is_err()
    );
    let directory = tempfile::tempdir().unwrap();
    assert!(
        validate_spec(&AgentSessionSpec {
            provider: "codex".into(),
            cwd: directory.path().to_str().unwrap().into(),
            config: StoredAgentConfig::default()
        })
        .is_err()
    );
}

#[test]
fn parses_grouped_options_and_rejects_malformed_state() {
    let state = state(&json!({"configOptions":[
        {"id":"models","category":"model","type":"select","currentValue":"auto","options":[
            {"group":"cursor","options":[{"value":"auto","name":"Auto"}]}]},
        {"id":"thinking","category":"thought_level","type":"select","currentValue":"high","options":[{"value":"high","name":"High"}]}
    ]})).unwrap();
    let catalog =
        catalog(&json!({"models":[{"value":"auto","name":"Auto","configOptions":state}]})).unwrap();
    let details = details(&state, &catalog).unwrap();
    assert_eq!(details.models[0]["defaultThinkingOptionId"], "high");
    assert_eq!(details.models[0]["thinkingOptions"][0]["id"], "high");
    assert_eq!(
        runtime("native", &state).thinking_option_id.as_deref(),
        Some("high")
    );
    for response in [
        json!({"configOptions":{}}),
        json!({"models":{}}),
        json!({"configOptions":[{"id":"model","type":"select","currentValue":"auto","options":[{"value":"auto"}]}]}),
    ] {
        assert!(super::state(&response).is_err());
    }
    assert!(super::state(&json!({"configOptions":[{"id":"model","category":"model","type":"select","options":null}]})).is_err());
    assert!(super::details(&json!([]), &[]).unwrap().models.is_empty());
}

#[test]
fn config_updates_retain_selectors_and_remove_obsolete_parameters() {
    let mut options = state(&json!({"models":{"currentModelId":"auto","availableModels":[{"modelId":"auto","name":"Auto"}]},
        "modes":{"currentModeId":"agent","availableModes":[{"id":"agent","name":"Agent"}]},
        "configOptions":[{"id":"fast","type":"select","currentValue":"false","options":[{"value":"false","name":"Off"},{"value":"true","name":"On"}]}]})).unwrap();
    merge(&mut options, &json!({"configOptions":[]})).unwrap();
    assert_eq!(runtime("native", &options).model.as_deref(), Some("auto"));
    assert_eq!(
        runtime("native", &options).mode_id.as_deref(),
        Some("agent")
    );
    assert!(option(&options, "fast").is_none());
    // A modern model picker must also survive a parameter-only notification.
    let mut modern = state(
        &json!({"configOptions":[{"id":"model","category":"model","type":"select",
        "currentValue":"auto","options":[{"value":"auto","name":"Auto"}]}]}),
    )
    .unwrap();
    merge(&mut modern, &json!({"configOptions":[]})).unwrap();
    assert_eq!(runtime("native", &modern).model.as_deref(), Some("auto"));
}

#[test]
fn model_catalog_is_authoritative_and_parameters_belong_to_each_model() {
    let options = state(&json!({"models":{"currentModelId":"auto","availableModels":[{"modelId":"auto","name":"Auto"}]}})).unwrap();
    let catalog = catalog(&json!({"models":[
        {"value":"auto","name":"Auto","configOptions":[]},
        {"value":"gpt[reasoning=high,fast=false]","name":"GPT","configOptions":[
            {"id":"thought_level","category":"thought_level","type":"select","currentValue":"high","options":[{"value":"high","name":"High"}]},
            {"id":"fast","type":"select","currentValue":"false","options":[{"value":"false","name":"Off"},{"value":"true","name":"On"}]}]}]})).unwrap();
    let details = details(&options, &catalog).unwrap();
    assert_eq!(details.models.len(), 2);
    assert_eq!(details.models[0]["thinkingOptions"], json!([]));
    assert!(details.models[0].get("defaultThinkingOptionId").is_none());
    assert_eq!(details.models[1]["id"], "gpt[reasoning=high,fast=false]");
    assert_eq!(details.models[1]["defaultThinkingOptionId"], "high");
    assert_eq!(details.models[1]["supportsFastMode"], true);
    assert_eq!(super::details(&options, &[]).unwrap().models.len(), 0);
    for response in [
        json!({}),
        json!({"models":{}}),
        json!({"models":[{"value":"auto","name":"Auto"}]}),
        json!({"models":[{"value":"auto","name":"Auto","configOptions":[]},{"value":"auto","name":"Duplicate","configOptions":[]}]}),
        json!({"models":[{"value":"auto","name":"Auto","configOptions":{}}]}),
    ] {
        assert!(super::catalog(&response).is_err());
    }
}
