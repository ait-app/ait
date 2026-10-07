use super::*;
use crate::ports::agent_session::AgentClient;
use crate::test_support::Fixture;

#[tokio::test]
async fn auxiliary_codex_request_is_ephemeral_tool_restricted_and_schema_constrained() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.cwd.join("metadata-response.json"),
        r#"{"title":"Fix metadata"}"#,
    )
    .unwrap();
    let mut spec = fixture.spec();
    spec.config.model = Some("metadata-model".into());
    let schema = json!({"type":"object","properties":{"title":{"type":"string"}},"required":["title"],"additionalProperties":false});
    let text = fixture
        .client()
        .generate_metadata(&spec, "Source material", &schema)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["title"],
        "Fix metadata"
    );
    let requests = fixture.requests();
    let start = requests
        .iter()
        .find(|request| request["method"] == "thread/start")
        .unwrap();
    assert_eq!(start["params"]["ephemeral"], true);
    assert_eq!(start["params"]["sandbox"], "read-only");
    assert_eq!(start["params"]["config"]["features"]["shell_tool"], false);
    assert!(
        start["params"]["config"]
            .get("model_reasoning_effort")
            .is_none()
    );
    for feature in [
        "apps",
        "plugins",
        "hooks",
        "goals",
        "memories",
        "code_mode",
        "code_mode_host",
        "code_mode_only",
        "view_image",
        "image_generation",
        "sleep_tool",
        "skill_search",
        "tool_suggest",
        "default_mode_request_user_input",
        "request_permissions_tool",
    ] {
        assert_eq!(start["params"]["config"]["features"][feature], false);
    }
    assert_eq!(start["params"]["config"]["notify"], json!([]));
    assert_eq!(
        start["params"]["config"]["tools"]["experimental_request_user_input"]["enabled"],
        false
    );
    assert_eq!(
        start["params"]["config"]["tools"]["update_plan"]["enabled"],
        false
    );
    let turn = requests
        .iter()
        .find(|request| request["method"] == "turn/start")
        .unwrap();
    assert_eq!(turn["params"]["outputSchema"], schema);
    assert_eq!(turn["params"]["model"], "metadata-model");
    assert!(!std::fs::read_dir(&fixture.cwd).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("native-session-")
    }));
    assert!(!std::fs::read_dir(&fixture.cwd).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("native-history-")
    }));
}

#[tokio::test]
async fn auxiliary_codex_protocol_errors_fail_without_foreground_sessions() {
    let fixture = Fixture::new();
    fixture.mode("error");
    assert!(
        fixture
            .client()
            .generate_metadata(&fixture.spec(), "source", &json!({}))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn auxiliary_codex_rejects_unexpected_native_tool_execution() {
    for kind in [
        "commandExecution",
        "imageView",
        "dynamicToolCall",
        "futureTool",
    ] {
        let fixture = Fixture::new();
        std::fs::write(
            fixture.cwd.join("metadata-response.json"),
            r#"{"title":"ignore tool result"}"#,
        )
        .unwrap();
        std::fs::write(
            fixture.cwd.join("metadata-tool.json"),
            json!({"type":kind,"id":"tool"}).to_string(),
        )
        .unwrap();
        assert_eq!(
            fixture
                .client()
                .generate_metadata(&fixture.spec(), "source", &json!({}))
                .await,
            Err(AgentSessionError::Rejected)
        );
    }
}
