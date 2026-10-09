use super::{AgentPersistenceHandle, AgentSessionError, native_handle};
use serde_json::{Value, json};

#[test]
fn protocol_failures_retain_static_diagnostics_without_changing_port_errors() {
    use super::super::{failure, types::Fault};

    for (fault, expected) in [
        (
            Fault::AgentCapabilityUnsupported,
            AgentSessionError::Rejected,
        ),
        (Fault::ProviderFailed, AgentSessionError::Failed),
        (Fault::RunRecoveryFailed, AgentSessionError::Failed),
        (Fault::SessionBusy, AgentSessionError::Failed),
        (Fault::RunCancelled, AgentSessionError::Failed),
        (Fault::RunLimitExceeded, AgentSessionError::Failed),
        (Fault::ToolUseRequiresAssistant, AgentSessionError::Failed),
        (Fault::ToolCallDuplicate, AgentSessionError::Failed),
    ] {
        let error = failure(fault, "OpenCode HTTP connection failed");
        assert_eq!(error.code, fault);
        assert_eq!(error.to_string(), "OpenCode HTTP connection failed");
        assert_eq!(super::error(error), expected);
    }
}

#[test]
fn native_handles_accept_encoded_and_legacy_objects_and_reject_invalid_data() {
    let saved = json!({"config":{},"model":"local/model","clients":{}});
    let mut handle = AgentPersistenceHandle {
        provider: "opencode".into(),
        session_id: "ses_one".into(),
        native_handle: Some(saved.clone()),
        metadata: None,
    };
    assert_eq!(native_handle(&handle).unwrap().as_ref(), &saved);
    handle.native_handle = Some(json!(saved.to_string()));
    assert_eq!(native_handle(&handle).unwrap().as_ref(), &saved);
    for invalid in [
        None,
        Some(Value::Null),
        Some(json!(42)),
        Some(json!("[]")),
        Some(json!("invalid")),
        Some(json!(" ".repeat(256 * 1024 + 1))),
    ] {
        handle.native_handle = invalid;
        assert_eq!(
            native_handle(&handle).unwrap_err(),
            AgentSessionError::Rejected
        );
    }
}

#[test]
fn permission_control_exposes_only_native_effects_and_keeps_unselected_rules() {
    use domain::agent_runtime::StoredAgentConfig;
    let mut config = StoredAgentConfig::default();
    assert_eq!(super::permission(&config), None);
    let feature = super::features(&config).remove(0);
    assert_eq!(feature["type"], "select");
    assert!(feature["value"].is_null());
    assert_eq!(
        feature["options"]
            .as_array()
            .unwrap()
            .iter()
            .map(|option| option["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["allow", "ask", "deny"]
    );
    for effect in ["allow", "ask", "deny"] {
        config.feature_values = Some(std::collections::BTreeMap::from([(
            "permission".into(),
            json!(effect),
        )]));
        super::validate(&config).unwrap();
        assert_eq!(super::permission(&config), Some(effect));
        assert_eq!(super::features(&config)[0]["value"], effect);
    }
    config.feature_values = Some(std::collections::BTreeMap::from([(
        "permission".into(),
        Value::Null,
    )]));
    super::validate(&config).unwrap();
    assert_eq!(super::permission(&config), None);
    assert!(super::features(&config)[0]["value"].is_null());
    for (key, value) in [
        ("permission", json!(true)),
        ("permission", json!("auto")),
        ("auto_approve", json!(true)),
        ("unknown", Value::Null),
    ] {
        config.feature_values = Some(std::collections::BTreeMap::from([(key.into(), value)]));
        assert!(super::validate(&config).is_err());
    }
}
