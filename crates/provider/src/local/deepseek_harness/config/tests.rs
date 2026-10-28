use serde_json::json;

use super::*;

#[test]
fn rejects_unsupported_controls_and_maps_mcp_transports() {
    for config in [
        StoredAgentConfig {
            mode_id: Some("full-access".to_owned()),
            ..StoredAgentConfig::default()
        },
        StoredAgentConfig {
            system_prompt: Some("ignored".to_owned()),
            ..StoredAgentConfig::default()
        },
        StoredAgentConfig {
            model: Some(String::new()),
            ..StoredAgentConfig::default()
        },
        StoredAgentConfig {
            feature_values: Some(std::collections::BTreeMap::from([(
                "fast_mode".to_owned(),
                json!(true),
            )])),
            ..StoredAgentConfig::default()
        },
        StoredAgentConfig {
            tool_policy: Some(json!({"preapproved":[]})),
            ..StoredAgentConfig::default()
        },
    ] {
        assert_eq!(validate(&config), Err(AgentSessionError::Rejected));
    }
    let config = StoredAgentConfig {
        mcp_servers: Some(std::collections::BTreeMap::from([
            (
                "local".to_owned(),
                json!({"type":"stdio","command":"/bin/tool","args":["one"],"env":{"KEY":"value"}}),
            ),
            (
                "remote".to_owned(),
                json!({"type":"http","url":"https://example.test/mcp","headers":{"X-Test":"value"}}),
            ),
        ])),
        ..StoredAgentConfig::default()
    };
    validate(&config).unwrap();
    assert_eq!(
        crate::local::acp_config::mcp_servers(&config),
        vec![
            json!({"name":"local","command":"/bin/tool","args":["one"],"env":[{"name":"KEY","value":"value"}]}),
            json!({"type":"http","name":"remote","url":"https://example.test/mcp","headers":[{"name":"X-Test","value":"value"}]}),
        ]
    );
    for server in [
        json!({"type":"sse","url":"https://example.test/mcp"}),
        json!({"type":"stdio","command":"relative-tool"}),
    ] {
        let mut invalid = config.clone();
        invalid
            .mcp_servers
            .as_mut()
            .unwrap()
            .insert("bad".to_owned(), server);
        assert!(validate(&invalid).is_err());
    }
}

#[test]
fn preserves_empty_provider_default_effort_and_rejects_malformed_catalogs() {
    let state = json!({"configOptions":[
        {"id":"model","type":"select","category":"model","currentValue":"opaque-model","options":[{"value":"opaque-model","name":"Model"}]},
        {"id":"effort","type":"select","category":"thought_level","currentValue":"","options":[{"value":"","name":"Provider default"},{"value":"high","name":"High"}]}
    ]});
    let options = super::state(&state).unwrap();
    let details = details(&options).unwrap();
    assert_eq!(details.models[0]["thinkingOptions"][0]["id"], "");
    assert_eq!(details.models[0]["defaultThinkingOptionId"], "");
    let runtime = runtime("native-one", &options);
    assert_eq!(runtime.thinking_option_id.as_deref(), Some(""));
    assert_eq!(runtime.model.as_deref(), Some("opaque-model"));
    assert!(super::state(&json!({})).is_err());
    assert!(super::state(&json!({"configOptions":[{"id":"model","type":"select","options":[{"value":1,"name":"bad"}]}]})).is_err());
}

#[test]
fn omits_absent_optional_model_fields_and_preserves_descriptions() {
    let options = json!([{
        "id":"model", "type":"select", "category":"model", "currentValue":"missing",
        "options":[
            {"value":"missing", "name":"Missing"},
            {"value":"null", "name":"Null", "description":null},
            {"value":"described", "name":"Described", "description":"Model description"}
        ]
    }]);
    let catalog = details(&options).unwrap();
    assert!(catalog.models[0].get("description").is_none());
    assert!(catalog.models[1].get("description").is_none());
    assert_eq!(catalog.models[2]["description"], "Model description");
    assert!(
        catalog
            .models
            .iter()
            .all(|model| model.get("defaultThinkingOptionId").is_none())
    );
}
