use super::*;
use crate::ports::agent_session::AgentClient;

fn fixture() -> (tempfile::TempDir, DeepSeekHarnessClient, AgentSessionSpec) {
    let root = tempfile::tempdir().unwrap();
    let program =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dsh_metadata.cjs");
    let configuration = root.path().join("configuration.txt");
    let text = "timer llm deepseek-llm-api-extensions session session-log-deepseek session-title agent agent-default-model llm-retry credentials llm-pi-ai llm-deepseek session-projection tools system-prompt agent-loop headless-runner user-custom-tools"
        .split_whitespace().fold(String::new(), |mut text, id| {
            writeln!(text, "- id: {id}").unwrap();
            text
        });
    std::fs::write(&configuration, text).unwrap();
    let mut client = DeepSeekHarnessClient::new(program);
    client.environment = serde_json::from_value(json!({
        "AIT_METADATA_FIXTURE_CONFIG":configuration.to_str().unwrap()
    }))
    .unwrap();
    let spec = AgentSessionSpec {
        provider: super::super::super::PROVIDER.into(),
        cwd: root.path().to_str().unwrap().into(),
        config: domain::agent_runtime::StoredAgentConfig {
            model: Some(json!(["deepseek-official", "fixture-model"]).to_string()),
            ..Default::default()
        },
    };
    (root, client, spec)
}

#[tokio::test]
async fn metadata_process_uses_an_isolated_patch_and_optional_reasoning_effort() {
    for effort in [None, Some("high")] {
        let (root, client, mut spec) = fixture();
        spec.config.thinking_option_id = effort.map(str::to_owned);

        let result = client
            .generate_summary(&spec, "Summarize fixture", &json!({"type":"object"}))
            .await
            .unwrap();

        assert_eq!(result.trim(), "{\"title\":\"fixture summary\"}");
        let patch: Value =
            serde_json::from_slice(&std::fs::read(root.path().join("observed.json")).unwrap())
                .unwrap();
        let entries = patch.as_array().unwrap();
        let task = entries
            .iter()
            .find(|entry| entry["id"] == "headless-runner" && entry.get("config").is_some())
            .unwrap();
        assert_eq!(task["inject"], json!([]));
        assert!(
            task["config"]["task"]
                .as_str()
                .unwrap()
                .contains("Summarize fixture")
        );
        assert!(
            entries
                .iter()
                .any(|entry| entry["id"] == "user-custom-tools" && entry["disabled"] == true)
        );
        let reasoning = entries
            .iter()
            .find(|entry| entry["config"].get("reasoningEffort").is_some());
        assert_eq!(
            reasoning.map(|entry| entry["config"]["reasoningEffort"].as_str().unwrap()),
            effort
        );
        assert!(!root.path().join("sessions").exists());
    }
}

#[tokio::test]
async fn invalid_metadata_models_fail_before_starting_the_native_process() {
    let (root, client, mut spec) = fixture();
    for model in [None, Some("broken"), Some("[]"), Some("[\"\",\"m\"]")] {
        spec.config.model = model.map(str::to_owned);
        assert_eq!(
            client.generate_summary(&spec, "source", &json!({})).await,
            Err(AgentSessionError::Rejected)
        );
    }
    spec.config.model = Some(json!(["other-provider", "m"]).to_string());
    spec.config.thinking_option_id = Some("high".into());
    assert_eq!(
        client.generate_summary(&spec, "source", &json!({})).await,
        Err(AgentSessionError::Rejected)
    );
    assert!(!root.path().join("observed.json").exists());
}
