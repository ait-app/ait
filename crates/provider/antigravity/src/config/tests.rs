use super::*;

#[test]
fn accepts_valid_modes_and_models() {
    for mode in [
        None,
        Some("default"),
        Some("plan"),
        Some("accept-edits"),
        Some("auto"),
        Some("full-access"),
    ] {
        let config = StoredAgentConfig {
            mode_id: mode.map(str::to_owned),
            model: Some("gemini-test-low".to_owned()),
            thinking_option_id: Some("high".to_owned()),
            ..StoredAgentConfig::default()
        };
        validate(&config).unwrap();
    }
    assert_eq!(modes().len(), 5);
}

fn servers(server: Value) -> StoredAgentConfig {
    StoredAgentConfig {
        mcp_servers: Some(std::collections::BTreeMap::from([(
            "server".to_owned(),
            server,
        )])),
        ..Default::default()
    }
}

#[test]
fn accepts_http_mcp_servers() {
    validate(&servers(
        json!({"type":"http","url":"http://127.0.0.1:7316/mcp/agents",
        "headers":{"Authorization":"Bearer private"}}),
    ))
    .unwrap();
    validate(&StoredAgentConfig {
        mcp_servers: Some(std::collections::BTreeMap::new()),
        ..Default::default()
    })
    .unwrap();
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
        servers(json!({})),
        servers(json!({"type":"stdio","command":"/usr/bin/server"})),
        servers(json!({"type":"sse","url":"https://legacy.test/sse"})),
        servers(json!({"type":"http","url":"ftp://docs.test/mcp"})),
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
