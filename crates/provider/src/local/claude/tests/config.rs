use super::*;
use crate::local::claude::config;

#[test]
fn rejects_unsupported_configuration_and_preserves_native_modes() {
    let client = ClaudeClient::new("missing-claude".into());
    assert_eq!(client.provider(), "claude");
    for mode in [
        "default",
        "plan",
        "acceptEdits",
        "auto",
        "bypassPermissions",
    ] {
        assert!(
            client
                .validate_config(&StoredAgentConfig {
                    mode_id: Some(mode.into()),
                    ..StoredAgentConfig::default()
                })
                .is_ok()
        );
    }
    for value in [
        json!({"model":""}),
        json!({"model":"bad\nmodel"}),
        json!({"thinkingOptionId":"bad"}),
        json!({"modeId":"full-access"}),
        json!({"featureValues":{"fast_mode":true}}),
        json!({"providerOptions":{"args":[]}}),
        json!({"toolPolicy":{}}),
        json!({"mcpServers":{"test":{}}}),
    ] {
        assert_eq!(
            client.validate_config(&serde_json::from_value(value).unwrap()),
            Err(AgentSessionError::Rejected)
        );
    }
    let settings = client.settings(&StoredAgentConfig::default());
    assert_eq!(settings["availableModes"].as_array().unwrap().len(), 5);
    assert_eq!(settings["capabilities"]["supportsStreaming"], true);
    assert_eq!(settings["capabilities"]["supportsSessionListing"], true);
    assert!(config::validate_spec(&spec(std::path::Path::new("relative"))).is_err());
}

#[test]
fn discovery_uses_cli_models_efforts_and_commands() {
    let response = json!({"models":[{"value":"default","displayName":"Default","supportedEffortLevels":["low","max"]},
        {"value":"custom","displayName":"Custom gateway"}],"commands":[{"name":"review","description":"Review"}]});
    let models = config::models(&response).unwrap();
    assert_eq!(models[0]["thinkingOptions"][1]["id"], "max");
    assert_eq!(models[0]["isDefault"], true);
    assert_eq!(models[1]["thinkingOptions"], json!([]));
    assert_eq!(config::commands(&response).unwrap()[0]["name"], "review");
    for bad in [
        json!({}),
        json!({"models":[]}),
        json!({"models":[{"value":"x"}]}),
        json!({"models":[{"value":"x","displayName":"X","supportedEffortLevels":[false]}]}),
    ] {
        assert!(config::models(&bad).is_err());
    }
    assert!(config::commands(&json!({"commands":[{}]})).is_err());
}

#[tokio::test]
async fn missing_executable_reports_safe_availability_and_diagnostic() {
    let mut client = ClaudeClient::new("/does-not-exist/claude".into());
    client.environment = serde_json::from_value(json!({
        "CLAUDE_CODE_OAUTH_TOKEN":"", "ANTHROPIC_API_KEY":"offline-fixture-key"
    }))
    .unwrap();
    assert!(!client.is_available().await.unwrap());
    assert!(client.diagnostic().await.unwrap().contains("unavailable"));
    let usage = client.usage().await.unwrap();
    assert_eq!(usage["status"], "unavailable");
    assert_eq!(usage["problem"]["kind"], "no_quota");
    assert!(!usage.to_string().contains("offline-fixture-key"));
}

#[test]
fn first_party_fast_and_thinking_capabilities_do_not_leak_to_custom_aliases() {
    for model in [
        "claude-opus-5",
        "claude-opus-4-6[1m]",
        "Opus 4.7",
        "claude-opus-4-8-20260101",
    ] {
        let config = serde_json::from_value(
            json!({"model":model,"thinkingOptionId":"off","featureValues":{"fast_mode":true}}),
        )
        .unwrap();
        config::validate(&config).unwrap();
        assert_eq!(config::features(&config, &[])[0]["value"], true);
    }
    for model in [
        "claude-sonnet-5",
        "custom-opus",
        "gateway/claude-opus-5",
        "opus",
    ] {
        let config =
            serde_json::from_value(json!({"model":model,"featureValues":{"fast_mode":true}}))
                .unwrap();
        assert_eq!(config::validate(&config), Err(AgentSessionError::Rejected));
        assert!(
            config::features(&config, &[])
                .iter()
                .all(|feature| feature["id"] != "fast_mode")
        );
    }
    let response = json!({"models":[{"value":"claude-opus-5","displayName":"Opus","supportedEffortLevels":["xhigh"]}]});
    let models = config::models(&response).unwrap();
    assert_eq!(
        models[0]["thinkingOptions"],
        json!([{"id":"off","label":"Off"},{"id":"xhigh","label":"xhigh"},{"id":"ultracode","label":"Ultra Code"}])
    );
}

#[test]
fn native_resolved_aliases_drive_discovery_validation_and_feature_controls() {
    let client = ClaudeClient::new("unused".into());
    let response = json!({"models":[{"value":"default","resolvedModel":"claude-opus-4-6","displayName":"Default","supportedEffortLevels":["high","max"]},
        {"value":"sonnet","resolvedModel":"claude-sonnet-4-6","displayName":"Sonnet","supportedEffortLevels":["high"]}]});
    client.remember_models(&response).unwrap();
    let config: StoredAgentConfig = serde_json::from_value(
        json!({"featureValues":{"fast_mode":true},"thinkingOptionId":"off"}),
    )
    .unwrap();
    client.validate_config(&config).unwrap();
    assert_eq!(client.settings(&config)["features"][0]["value"], true);
    let models = config::models(&response).unwrap();
    assert_eq!(models[0]["supportsFastMode"], true);
    assert_eq!(models[1]["supportsFastMode"], false);
    assert_eq!(models[1]["thinkingOptions"][0]["id"], "off");
    assert!(
        client
            .validate_config(&StoredAgentConfig {
                model: Some("sonnet".into()),
                ..config
            })
            .is_err()
    );
}

#[test]
fn advisor_role_accepts_keywords_aliases_and_model_identifiers() {
    for selection in ["native", "unset", "opus", "claude-opus-5-1-20260101"] {
        let config = serde_json::from_value(
            json!({"model":"sonnet","featureValues":{"modelRole.advisor":selection}}),
        )
        .unwrap();
        config::validate_shape(&config).unwrap();
        assert_eq!(config::advisor(&config), expected_advisor(selection));
    }
    for selection in [
        json!(true),
        json!(1),
        json!(""),
        json!("bad\nmodel"),
        json!(null),
    ] {
        let config = serde_json::from_value(
            json!({"model":"sonnet","featureValues":{"modelRole.advisor":selection}}),
        )
        .unwrap();
        assert_eq!(
            config::validate_shape(&config),
            Err(AgentSessionError::Rejected),
            "{selection}"
        );
    }
    let unknown =
        serde_json::from_value(json!({"featureValues":{"claudeAdvisor":"opus"}})).unwrap();
    assert_eq!(
        config::validate_shape(&unknown),
        Err(AgentSessionError::Rejected)
    );
}

fn expected_advisor(selection: &str) -> config::Advisor<'_> {
    match selection {
        "native" => config::Advisor::Native,
        "unset" => config::Advisor::Disabled,
        model => config::Advisor::Model(model),
    }
}

#[test]
fn advisor_feature_offers_inheritance_unassignment_and_every_discovered_model() {
    let catalog = vec![
        json!({"id":"sonnet","label":"Sonnet"}),
        json!({"id":"opus","label":"Opus"}),
    ];
    let none = config::features(&StoredAgentConfig::default(), &catalog);
    let advisor = none
        .iter()
        .find(|feature| feature["id"] == "modelRole.advisor")
        .expect("advisor feature");
    assert_eq!(advisor["type"], "select");
    assert_eq!(advisor["value"], "native");
    let ids = advisor["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|option| option["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(ids, ["native", "unset", "sonnet", "opus"]);
    assert_eq!(advisor["options"][0]["isDefault"], true);

    let selected =
        serde_json::from_value(json!({"featureValues":{"modelRole.advisor":"opus"}})).unwrap();
    let features = config::features(&selected, &catalog);
    assert_eq!(
        features
            .iter()
            .find(|feature| feature["id"] == "modelRole.advisor")
            .unwrap()["value"],
        "opus"
    );
    assert!(
        features
            .iter()
            .all(|feature| feature["id"] != "modelPreset"),
        "Claude has no native preset catalog; the client composes presets instead"
    );
}

#[test]
fn advisor_must_outrank_the_main_model_and_clear_the_minimum_tier() {
    for (model, advisor) in [
        ("opus-5", "sonnet-4-6"),
        ("opus-4-8", "opus-4-6"),
        ("sonnet-5", "haiku-4-5"),
        ("haiku-5-5", "sonnet-4-6"),
    ] {
        let config = serde_json::from_value(
            json!({"model":model,"featureValues":{"modelRole.advisor":advisor}}),
        )
        .unwrap();
        assert_eq!(
            config::validate(&config),
            Err(AgentSessionError::Rejected),
            "{advisor} cannot serve {model}"
        );
    }
    for (model, advisor) in [
        ("sonnet-4-6", "opus-5"),
        ("opus-5", "opus-5"),
        ("sonnet-5", "sonnet-5"),
        ("opus-4-6", "claude-opus-5-5-20260601"),
    ] {
        let config = serde_json::from_value(
            json!({"model":model,"featureValues":{"modelRole.advisor":advisor}}),
        )
        .unwrap();
        config::validate(&config).unwrap();
    }
    // Inherited and disabled advisors are never a capability decision.
    for advisor in ["native", "unset"] {
        let config = serde_json::from_value(
            json!({"model":"sonnet-5","featureValues":{"modelRole.advisor":advisor}}),
        )
        .unwrap();
        config::validate(&config).unwrap();
    }
    // Unknown names stay with the CLI, which degrades them to a warning.
    let gateway = serde_json::from_value(json!({"model":"sonnet-5",
            "featureValues":{"modelRole.advisor":"gateway/custom-advisor"}}))
    .unwrap();
    config::validate(&gateway).unwrap();
}

#[test]
fn a_saved_advisor_stays_selectable_after_it_leaves_the_catalog() {
    let config = serde_json::from_value(json!({"model":"sonnet-4-6",
        "featureValues":{"modelRole.advisor":"claude-opus-4-6"}}))
    .unwrap();
    let features = config::features(&config, &[]);
    let advisor = features
        .iter()
        .find(|feature| feature["id"] == "modelRole.advisor")
        .unwrap();
    assert_eq!(advisor["value"], "claude-opus-4-6");
    assert_eq!(
        advisor["options"].as_array().unwrap().last().unwrap()["id"],
        "claude-opus-4-6"
    );
}
