use serde_json::json;

use super::*;

#[test]
fn initial_agent_creation_accepts_deepseek_harness() {
    let input =
        json!({"config":{"provider":"deepseek-harness","cwd":"/source"},"initialPrompt":"hello"});
    let intent = validate(&input).unwrap();
    assert_eq!(intent["config"]["provider"], "deepseek-harness");
}

#[test]
fn initial_agent_validation_binds_environment_without_retaining_secrets() {
    let input = json!({"config":{"provider":"codex","cwd":"/source"},
        "initialPrompt":"hello","env":{"PRIVATE_VALUE":"ephemeral-value"}});
    let intent = validate(&input).unwrap();
    assert!(!intent.to_string().contains("ephemeral-value"));
    assert!(intent["env"]["sha256"].is_string());
    assert_eq!(intent["initialPrompt"], "hello");
    for field in [
        "workspaceId",
        "git",
        "worktree",
        "idempotencyKey",
        "subscribe",
    ] {
        let mut invalid = input.clone();
        invalid[field] = json!("unsupported");
        assert!(validate(&invalid).is_err());
    }
    let mut invalid = input;
    invalid["agentId"] = json!("not-a-uuid");
    assert!(validate(&invalid).is_err());
}
