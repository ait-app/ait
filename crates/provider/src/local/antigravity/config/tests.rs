use super::*;

#[test]
fn maps_only_explicit_full_access_to_skip_permissions() {
    for mode in [
        None,
        Some("default"),
        Some("plan"),
        Some("accept-edits"),
        Some("full-access"),
    ] {
        let config = StoredAgentConfig {
            mode_id: mode.map(str::to_owned),
            model: Some("gemini-test-low".to_owned()),
            thinking_option_id: Some("high".to_owned()),
            ..StoredAgentConfig::default()
        };
        validate(&config).unwrap();
        let args = arguments(&config, Some("conversation-id"));
        assert_eq!(
            args.iter()
                .any(|arg| arg == "--dangerously-skip-permissions"),
            mode == Some("full-access")
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--conversation", "conversation-id"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--model", "gemini-test-low"])
        );
        assert!(args.windows(2).any(|pair| pair == ["--effort", "high"]));
        assert!(args.iter().any(|arg| arg == "--disable-slash-commands"));
    }
}

#[test]
fn rejects_settings_that_cannot_be_delivered_natively() {
    for config in [
        StoredAgentConfig {
            mode_id: Some("always-ask".to_owned()),
            ..Default::default()
        },
        StoredAgentConfig {
            model: Some("bad\nmodel".to_owned()),
            ..Default::default()
        },
        StoredAgentConfig {
            thinking_option_id: Some("unknown".to_owned()),
            ..Default::default()
        },
        StoredAgentConfig {
            system_prompt: Some("instructions".to_owned()),
            ..Default::default()
        },
        StoredAgentConfig {
            tool_policy: Some(json!({"allow":[]})),
            ..Default::default()
        },
        StoredAgentConfig {
            mcp_servers: Some(std::collections::BTreeMap::from([(
                "server".to_owned(),
                json!({}),
            )])),
            ..Default::default()
        },
        StoredAgentConfig {
            feature_values: Some(std::collections::BTreeMap::from([(
                "fast".to_owned(),
                json!(true),
            )])),
            ..Default::default()
        },
        StoredAgentConfig {
            provider_options: Some(std::collections::BTreeMap::from([(
                "args".to_owned(),
                json!([]),
            )])),
            ..Default::default()
        },
    ] {
        assert_eq!(validate(&config), Err(AgentSessionError::Rejected));
    }
    assert_eq!(
        validate_directory("relative"),
        Err(AgentSessionError::Rejected)
    );
}

#[test]
fn runtime_reports_only_native_model_facts() {
    let config = StoredAgentConfig::default();
    let info = runtime("id", &config, &json!({"permission_mode":"request-review"}));
    assert_eq!(info.model, None);
    assert_eq!(info.mode_id.as_deref(), Some("default"));
    assert_eq!(info.extra.unwrap()["permissionMode"], "request-review");
    assert_eq!(modes().len(), 4);
}
