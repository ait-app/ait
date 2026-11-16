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

#[cfg(target_os = "linux")]
#[tokio::test]
async fn sandboxed_claude_session_uses_the_config_override_and_native_environment() {
    let (root, client, mut spec) = fixture();
    std::fs::create_dir_all(root.path().join(".git")).unwrap();
    std::fs::write(root.path().join("capture-environment"), "").unwrap();
    let record = serde_json::from_value(json!({
        "workspaceId":"wks_0123456789abcdef", "projectId":"prj_1", "kind":"worktree",
        "cwd":root.path(), "worktreeRoot":root.path(), "displayName":"sandbox",
        "createdAt":"2026-01-01T00:00:00Z", "updatedAt":"2026-01-01T00:00:00Z",
        "sandbox":"worktree_writes", "archivedAt":null
    }))
    .unwrap();
    spec.confinement = domain::workspace::sandbox::WriteConfinement::for_workspace(&record);
    let environment = serde_json::from_value(json!({
        "HOME":root.path().join("home"),
        "CLAUDE_CONFIG_DIR":root.path().join("custom-config"),
        "AIT_TEST_AGENT_ENV":"sandbox-fixture"
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
    session.close().await.unwrap();
    assert!(root.path().join("custom-config/projects").is_dir());
    let rows = captured(root.path());
    assert!(!rows.is_empty());
    assert!(rows.iter().all(|row| row["value"] == "sandbox-fixture"));
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn sandboxed_home_only_override_seeds_existing_global_settings() {
    let (root, mut client, mut spec) = fixture();
    client.config_dir = None;
    let home = root.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(root.path().join(".git")).unwrap();
    std::fs::write(home.join(".claude.json"), "{\"fixtureSetting\":true}").unwrap();
    let record = serde_json::from_value(json!({
        "workspaceId":"wks_0123456789abcdef", "projectId":"prj_1", "kind":"worktree",
        "cwd":root.path(), "worktreeRoot":root.path(), "displayName":"sandbox",
        "createdAt":"2026-01-01T00:00:00Z", "updatedAt":"2026-01-01T00:00:00Z",
        "sandbox":"worktree_writes", "archivedAt":null
    }))
    .unwrap();
    spec.confinement = domain::workspace::sandbox::WriteConfinement::for_workspace(&record);
    let environment = serde_json::from_value(json!({"HOME":home})).unwrap();
    let mut session = client
        .create_session_with_environment(&spec, &environment)
        .await
        .unwrap();
    session.close().await.unwrap();
    assert_eq!(
        std::fs::read_to_string(home.join(".claude/.claude.json")).unwrap(),
        "{\"fixtureSetting\":true}"
    );
}
