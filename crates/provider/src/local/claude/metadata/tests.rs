use super::super::tests::fixture;
use super::*;
use crate::ports::agent_session::AgentClient;

#[tokio::test]
async fn auxiliary_claude_request_disables_tools_mcp_hooks_and_history() {
    let (root, client, mut spec) = fixture();
    spec.config.model = Some("haiku".into());
    std::fs::write(
        root.path().join("metadata-response.json"),
        r#"{"message":"Fix metadata"}"#,
    )
    .unwrap();
    let schema = json!({"type":"object","properties":{"message":{"type":"string"}},"required":["message"],"additionalProperties":false});
    let result = client
        .generate_summary(&spec, "source", &schema)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&result).unwrap()["message"],
        "Fix metadata"
    );
    let args: Vec<String> =
        serde_json::from_slice(&std::fs::read(root.path().join("claude-args.json")).unwrap())
            .unwrap();
    for argument in [
        "--no-session-persistence",
        "--tools=",
        "--strict-mcp-config",
        "--settings={\"disableAllHooks\":true}",
        "--model=haiku",
    ] {
        assert!(args.iter().any(|arg| arg == argument), "missing {argument}");
    }
    assert!(!root.path().join("config/projects").exists());
}

#[tokio::test]
async fn auxiliary_claude_errors_and_timeout_release_native_process() {
    let (_root, client, spec) = fixture();
    assert!(
        client
            .generate_summary(&spec, "error-result", &json!({}))
            .await
            .is_err()
    );
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            client.generate_summary(&spec, "hold", &json!({}))
        )
        .await
        .is_err()
    );
}
