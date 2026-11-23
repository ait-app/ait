use std::path::PathBuf;

use serde_json::{Value, json};

use super::*;
use provider_core::ports::agent_session::AgentClient;

fn fixture() -> (tempfile::TempDir, MiniMaxClient, AgentSessionSpec) {
    let root = tempfile::tempdir().unwrap();
    let client = MiniMaxClient::new(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/tests/fixtures/acp.py"),
    )
    .with_image_directory(root.path().join("images"));
    let spec = AgentSessionSpec {
        provider: PROVIDER.into(),
        cwd: root.path().to_str().unwrap().into(),
        config: domain::agent_runtime::StoredAgentConfig::default(),
        confinement: None,
    };
    (root, client, spec)
}

#[test]
fn summary_model_selects_m3_1_flash_with_low_effort() {
    let client = MiniMaxClient::new("mcode".into());
    assert!(client.supports_summary_generation());

    let native_models = vec![
        json!({
            "id": "m:minimax:MiniMax-M3",
            "label": "M3",
            "isSelectable": true,
            "thinkingOptions": [{"id": "off"}, {"id": "thinking"}]
        }),
        json!({
            "id": "m:minimax:MiniMax-M3.1-Flash",
            "label": "M3.1-Flash",
            "isSelectable": true,
            "thinkingOptions": [
                {"id": "thinking:default"},
                {"id": "thinking:low"},
                {"id": "thinking:high"}
            ]
        }),
    ];
    let selected = client.summary_model(&native_models).unwrap();
    assert_eq!(selected.provider, PROVIDER);
    assert_eq!(
        selected.model.as_deref(),
        Some("m:minimax:MiniMax-M3.1-Flash")
    );
    assert_eq!(selected.thinking_option_id.as_deref(), Some("thinking:low"));

    // Label-based fallback and non-variant catalog formats also match.
    let fixture_models = vec![
        json!({"id": "minimax/pro", "label": "Pro", "isSelectable": true}),
        json!({
            "id": "minimax/flash",
            "label": "Flash",
            "isSelectable": true,
            "thinkingOptions": [{"id": "default"}, {"id": "low"}]
        }),
    ];
    let fallback = client.summary_model(&fixture_models).unwrap();
    assert_eq!(fallback.model.as_deref(), Some("minimax/flash"));
    assert_eq!(fallback.thinking_option_id.as_deref(), Some("low"));

    assert!(client.summary_model(&native_models[..1]).is_none());
}

#[test]
fn exec_model_args_converts_acp_and_slash_identities() {
    assert_eq!(
        exec_model_args(Some("m:minimax:MiniMax-M3.1-Flash"), Some("thinking:low")).unwrap(),
        (
            Some("minimax/MiniMax-M3.1-Flash#thinking".into()),
            Some("low".into())
        )
    );
    assert_eq!(
        exec_model_args(
            Some("m:minimax:MiniMax-M3.1-Flash:v:thinking"),
            Some("default")
        )
        .unwrap(),
        (Some("minimax/MiniMax-M3.1-Flash#thinking".into()), None)
    );
    assert_eq!(
        exec_model_args(Some("minimax/flash"), Some("low")).unwrap(),
        (Some("minimax/flash".into()), Some("low".into()))
    );
    assert_eq!(
        exec_model_args(Some("invalid-model"), None),
        Err(AgentSessionError::Rejected)
    );
}

#[tokio::test]
async fn generate_summary_runs_isolated_exec_and_propagates_failures() {
    let (root, client, mut spec) = fixture();
    spec.config.model = Some("m:minimax:MiniMax-M3.1-Flash".into());
    spec.config.thinking_option_id = Some("thinking:low".into());
    let schema =
        json!({"type":"object","properties":{"title":{"type":"string"}},"required":["title"]});

    let output = client
        .generate_summary(&spec, "Summarize this task", &schema)
        .await
        .unwrap();
    let parsed: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(parsed["title"], "MiniMax summary");

    let recorded: Value =
        serde_json::from_str(&std::fs::read_to_string(root.path().join("exec-args.json")).unwrap())
            .unwrap();
    let args = recorded["args"].as_array().unwrap();
    assert!(args.iter().any(|arg| arg == "--permission"));
    assert!(args.iter().any(|arg| arg == "off"));
    assert!(args.iter().any(|arg| arg == "--max-steps"));
    assert!(args.iter().any(|arg| arg == "1"));
    assert!(args.iter().any(|arg| arg == "--mode"));
    assert!(args.iter().any(|arg| arg == "lightweight"));
    assert!(
        args.iter()
            .any(|arg| arg == "minimax/MiniMax-M3.1-Flash#thinking")
    );
    assert!(args.iter().any(|arg| arg == "low"));
    assert_eq!(recorded["stdin"], "Summarize this task");

    std::fs::write(root.path().join("scenario"), "exec-fail").unwrap();
    assert_eq!(
        client
            .generate_summary(&spec, "Summarize this task", &schema)
            .await,
        Err(AgentSessionError::Failed)
    );

    assert_eq!(
        client
            .generate_summary(&spec, "Summarize this task", &json!("not-an-object"))
            .await,
        Err(AgentSessionError::Rejected)
    );
}
