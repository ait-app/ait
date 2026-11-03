use super::*;

#[tokio::test]
async fn per_agent_environment_reaches_claude_and_does_not_change_the_shared_client() {
    let (root, client, spec) = fixture();
    std::fs::write(root.path().join("capture-environment"), "").unwrap();
    let environment = serde_json::from_value(serde_json::json!({
        "AIT_TEST_AGENT_ENV":"claude-private-fixture", "CLAUDECODE":"nested-client",
        "CLAUDE_CONFIG_DIR":root.path().join("custom-config"),
        "CLAUDE_CODE_ENTRYPOINT":"sdk-cli",
        "CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING":"false"
    }))
    .unwrap();
    let mut session = client
        .create_session_with_environment(&spec, &environment)
        .await
        .unwrap();
    session
        .start_turn("environment", &spec.config)
        .await
        .unwrap();
    assert_eq!(finish(session.as_mut()).await, "Claude: environment");
    assert!(!format!("{session:?}").contains("claude-private-fixture"));
    assert!(
        !serde_json::to_string(&session.persistence())
            .unwrap()
            .contains("claude-private-fixture")
    );
    session.close().await.unwrap();
    assert!(root.path().join("custom-config/projects").is_dir());
    assert!(!root.path().join("config/projects").exists());
    let rows = captured(root.path());
    assert!(
        rows.iter()
            .all(|row| row["value"] == "claude-private-fixture")
    );
    assert!(
        rows.iter()
            .all(|row| row["nested"].is_null() && row["checkpoint"] == "true")
    );
    assert!(rows.iter().all(|row| row["entrypoint"] == "ait"));
    std::fs::write(root.path().join("native-environment.jsonl"), "").unwrap();
    let mut other = client.create_session(&spec).await.unwrap();
    other.close().await.unwrap();
    assert!(
        captured(root.path())
            .iter()
            .all(|row| row["value"].is_null() && row["entrypoint"] == "ait")
    );
}

fn captured(root: &std::path::Path) -> Vec<Value> {
    std::fs::read_to_string(root.join("native-environment.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
