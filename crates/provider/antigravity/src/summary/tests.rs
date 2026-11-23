use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::*;
use provider_core::ports::agent_session::AgentClient;
use provider_core::ports::environment::AgentEnvironment;

fn fixture() -> (tempfile::TempDir, AntigravityClient, AgentSessionSpec) {
    let directory = tempfile::tempdir().unwrap();
    let cwd = directory.path().canonicalize().unwrap();
    let program =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/fixtures/agy.py");
    let mut client = AntigravityClient::new(program).with_image_directory(cwd.join("images"));
    client.environment = AgentEnvironment::try_from(BTreeMap::from([(
        "AGY_FIXTURE_LOG".to_owned(),
        cwd.join("requests.jsonl").to_str().unwrap().to_owned(),
    )]))
    .unwrap();
    let spec = AgentSessionSpec {
        provider: PROVIDER.to_owned(),
        cwd: cwd.to_str().unwrap().to_owned(),
        config: domain::agent_runtime::StoredAgentConfig::default(),
        confinement: None,
    };
    (directory, client, spec)
}

#[test]
fn summary_model_prefers_hydrogen_then_flash_then_gpt_oss_with_low_effort() {
    let client = AntigravityClient::new("agy".into());
    assert!(client.supports_summary_generation());
    let models = vec![
        json!({"id":"gemini-4-neon","label":"Gemini 4 Neon","isSelectable":true,"thinkingOptions":[{"id":"low"},{"id":"high"}]}),
        json!({"id":"gpt-oss-120b","label":"GPT-OSS 120B","isSelectable":true,"thinkingOptions":[{"id":"medium"}]}),
        json!({"id":"gemini-3.8-flash","label":"Gemini 3.8 Flash","isSelectable":true,"thinkingOptions":[{"id":"low"},{"id":"high"}]}),
        json!({"id":"gemini-4-hydrogen","label":"Gemini 4 Hydrogen","isSelectable":true,"thinkingOptions":[{"id":"low"},{"id":"medium"}]}),
    ];

    let hydrogen = client.summary_model(&models).unwrap();
    assert_eq!(hydrogen.provider, PROVIDER);
    assert_eq!(hydrogen.model.as_deref(), Some("gemini-4-hydrogen"));
    assert_eq!(hydrogen.thinking_option_id.as_deref(), Some("low"));

    let flash = client.summary_model(&models[..3]).unwrap();
    assert_eq!(flash.model.as_deref(), Some("gemini-3.8-flash"));
    assert_eq!(flash.thinking_option_id.as_deref(), Some("low"));

    let oss = client.summary_model(&models[..2]).unwrap();
    assert_eq!(oss.model.as_deref(), Some("gpt-oss-120b"));
    assert_eq!(oss.thinking_option_id, None);

    assert!(client.summary_model(&models[..1]).is_none());
}

#[tokio::test]
async fn generate_summary_enforces_schema_denies_tools_and_rejects_tool_steps() {
    let (directory, mut client, mut spec) = fixture();
    spec.config.model = Some("gemini-test".into());
    spec.config.thinking_option_id = Some("low".into());
    let schema =
        json!({"type":"object","properties":{"title":{"type":"string"}},"required":["title"]});

    let output = client
        .generate_summary(&spec, "Summarize this task", &schema)
        .await
        .unwrap();
    let parsed: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(parsed["title"], "Fixture summary");

    let logs: Vec<Value> = std::fs::read_to_string(directory.path().join("requests.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let launch_args = logs
        .iter()
        .find(|entry| {
            entry["args"]
                .as_array()
                .is_some_and(|args| args.iter().any(|arg| arg == "--json-schema"))
        })
        .unwrap()["args"]
        .as_array()
        .unwrap();
    assert!(launch_args.iter().any(|arg| arg == "--model"));
    assert!(launch_args.iter().any(|arg| arg == "gemini-test"));
    assert!(launch_args.iter().any(|arg| arg == "--effort"));
    assert!(launch_args.iter().any(|arg| arg == "low"));
    assert!(launch_args.iter().any(|arg| arg == DENY_ALL_SETTINGS));

    // Any tool execution during metadata generation is rejected.
    client.environment = AgentEnvironment::try_from(BTreeMap::from([(
        "AGY_FIXTURE_SCENARIO".to_owned(),
        "summary-tool".to_owned(),
    )]))
    .unwrap();
    assert_eq!(
        client
            .generate_summary(&spec, "Summarize this task", &schema)
            .await,
        Err(AgentSessionError::Rejected)
    );

    // Non-object schemas are rejected before launching any process.
    assert_eq!(
        client
            .generate_summary(&spec, "Summarize this task", &json!("invalid"))
            .await,
        Err(AgentSessionError::Rejected)
    );
}
