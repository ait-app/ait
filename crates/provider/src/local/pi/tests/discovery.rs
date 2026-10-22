use super::*;

#[tokio::test]
async fn slow_state_reads_preserve_the_same_catalog_defaults() {
    let (_directory, client, spec) = fixture("");
    let expected = client.discover(&spec.cwd).await.unwrap();
    assert_eq!(expected.models[0]["isDefault"], true);
    assert_eq!(expected.models[0]["defaultThinkingOptionId"], "medium");
    let (directory, client, spec) = fixture("discovery-state-slow");

    let details = client.discover(&spec.cwd).await.unwrap();

    assert_eq!(details.models, expected.models);
    assert_eq!(details.modes, expected.modes);
    assert_eq!(details.features, expected.features);
    assert!(!directory.path().join("session.jsonl").exists());
}

#[tokio::test]
async fn state_read_failures_fail_discovery_instead_of_dropping_defaults() {
    for scenario in [
        "discovery-state-rejected",
        "discovery-state-timeout",
        "discovery-state-exit",
        "discovery-state-malformed",
    ] {
        let (directory, mut client, spec) = fixture(scenario);
        client.deadline = Duration::from_secs(2);

        assert!(client.discover(&spec.cwd).await.is_err(), "{scenario}");
        assert!(!directory.path().join("session.jsonl").exists());
    }
}

#[tokio::test]
async fn successful_state_without_a_native_model_leaves_defaults_unspecified() {
    let (_directory, client, spec) = fixture("discovery-state-missing-model");

    let details = client.discover(&spec.cwd).await.unwrap();

    assert_eq!(details.models[0]["id"], "fixture/model");
    assert_eq!(details.models[0]["isDefault"], false);
    assert!(details.models[0].get("defaultThinkingOptionId").is_none());
}

#[tokio::test]
async fn selection_validation_reads_only_the_model_catalog() {
    for scenario in [
        "",
        "discovery-state-slow",
        "discovery-state-rejected",
        "discovery-state-timeout",
        "discovery-state-exit",
        "discovery-state-malformed",
        "discovery-state-missing-model",
    ] {
        let (directory, client, mut spec) = fixture(scenario);
        spec.config.model = Some("fixture/model".into());
        for level in ["high", "max", "future"] {
            spec.config.thinking_option_id = Some(level.into());
            client.validate_selection(&spec).await.unwrap();
        }
        spec.config.model = Some("fixture/missing".into());
        assert_eq!(
            client.validate_selection(&spec).await,
            Err(AgentSessionError::Rejected),
            "{scenario}"
        );

        let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
        let commands: Vec<Value> = requests
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap()["type"].clone())
            .collect();
        assert_eq!(
            commands,
            vec![json!("get_available_models"); 4],
            "{scenario}"
        );
        assert!(!directory.path().join("session.jsonl").exists());
    }
}

#[tokio::test]
async fn unavailable_or_invalid_models_fail_discovery_and_selection_validation() {
    for scenario in ["discovery-models-rejected", "bad-models", "bad-model-label"] {
        let (_directory, client, mut spec) = fixture(scenario);
        spec.config.model = Some("fixture/model".into());

        assert!(client.discover(&spec.cwd).await.is_err(), "{scenario}");
        assert!(
            client.validate_selection(&spec).await.is_err(),
            "{scenario}"
        );
    }
}
