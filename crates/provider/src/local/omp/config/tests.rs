use super::*;
use std::collections::BTreeMap;

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
    assert_eq!(features(&StoredAgentConfig::default()).len(), 4);
}
