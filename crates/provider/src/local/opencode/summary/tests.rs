use super::*;

#[test]
fn configurations_disable_tools_for_both_native_versions() {
    let first = configuration(Version::V1, "private");
    assert_eq!(first["agent"]["private"]["steps"], 1);
    assert_eq!(first["permission"]["*"], "deny");
    let second = configuration(Version::V2, "private");
    assert_eq!(second["agents"]["private"]["steps"], 1);
    assert_eq!(second["permissions"][0]["effect"], "deny");
}

#[test]
fn accepts_only_completed_assistant_text_and_rejects_tool_results() {
    let text =
        json!({"type":"assistant","time":{"completed":1},"content":[{"type":"text","text":"{}"}]});
    assert_eq!(response(Version::V2, &[text]).unwrap(), Some("{}".into()));
    assert_eq!(
        response(Version::V2, &[json!({"type":"user","text":"source"})]).unwrap(),
        None
    );
    assert!(
        response(
            Version::V2,
            &[json!({"type":"assistant","time":{"completed":1},"content":[{"type":"tool"}]})]
        )
        .is_err()
    );
    let text = json!({"info":{"role":"assistant","time":{"completed":1}},"parts":[{"type":"text","text":"{}"}]});
    assert_eq!(response(Version::V1, &[text]).unwrap(), Some("{}".into()));
    assert!(
        response(
            Version::V1,
            &[json!({"info":{"role":"assistant","error":{}}})]
        )
        .is_err()
    );
}

#[test]
fn does_not_accept_partial_or_null_completion() {
    assert_eq!(
        response(
            Version::V1,
            &[json!({"info":{"role":"assistant"},"parts":[{"type":"text","text":"partial"}]})]
        )
        .unwrap(),
        None
    );
    assert_eq!(response(Version::V2, &[json!({"type":"assistant","time":{"completed":null},"content":[{"type":"text","text":"partial"}]})]).unwrap(), None);
}

#[test]
fn session_creation_respects_versioned_native_schemas() {
    let spec = AgentSessionSpec {
        provider: "opencode".into(),
        cwd: "/tmp/workspace".into(),
        config: domain::agent_runtime::StoredAgentConfig::default(),
    };
    let model = json!({"providerID":"google", "id":"gemini-flash"});
    assert_eq!(
        create_parameters(Version::V1, &spec, "private", &model),
        json!({})
    );
    let second = create_parameters(Version::V2, &spec, "private", &model);
    assert_eq!(second["agent"], "private");
    assert_eq!(second["model"], model);
    assert!(second["id"].as_str().unwrap().starts_with("ses_"));
    assert_eq!(second["permission"][0]["action"], "deny");
}
