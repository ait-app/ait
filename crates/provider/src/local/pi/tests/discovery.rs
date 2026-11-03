use super::*;

#[test]
fn model_labels_prefer_names_and_fall_back_to_unqualified_ids() {
    let data = json!({"models":[
        {"provider":"backend","id":"named","name":"Display name"},
        {"provider":"backend","id":"unnamed"}
    ]});

    let ids = config::model_ids(&data).unwrap();
    let models = config::models(&data, &Value::Null).unwrap();

    assert_eq!(ids, ["backend/named", "backend/unnamed"]);
    assert_eq!(models[0]["id"], ids[0]);
    assert_eq!(models[1]["id"], ids[1]);
    assert_eq!(models[0]["label"], "Display name");
    assert_eq!(models[1]["label"], "unnamed");
}

#[test]
fn invalid_explicit_model_names_fail_instead_of_falling_back_to_ids() {
    for name in [
        Value::Null,
        json!(false),
        json!(42),
        json!([]),
        json!({}),
        json!(""),
        json!("bad\nlabel"),
        json!("x".repeat(1025)),
    ] {
        let data = json!({"models":[{"provider":"backend","id":"model","name":name}]});

        assert_eq!(config::model_ids(&data), Err(AgentSessionError::Failed));
        assert_eq!(
            config::models(&data, &Value::Null),
            Err(AgentSessionError::Failed)
        );
    }
}

#[test]
fn duplicate_qualified_model_ids_fail_catalog_and_id_validation() {
    let first = json!({"provider":"a","id":"b/c"});
    assert_eq!(
        config::model_ids(&json!({"models":[first.clone()]})).unwrap(),
        ["a/b/c"]
    );
    for second in [first.clone(), json!({"provider":"a/b","id":"c"})] {
        let data = json!({"models":[first.clone(), second]});
        assert_eq!(config::model_ids(&data), Err(AgentSessionError::Failed));
        assert_eq!(
            config::models(&data, &Value::Null),
            Err(AgentSessionError::Failed)
        );
    }
}

#[tokio::test]
async fn discovery_seeds_each_model_from_the_ambient_thinking_level() {
    let (directory, client, spec) = fixture("discovery-thinking-defaults");

    let details = client.discover(&spec.cwd).await.unwrap();

    let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    let commands: Vec<Value> = requests
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap()["type"].clone())
        .collect();
    assert_eq!(commands, ["get_available_models", "get_state"]);
    let arguments: Value = serde_json::from_str(
        &std::fs::read_to_string(directory.path().join("arguments.json")).unwrap(),
    )
    .unwrap();
    assert!(
        arguments
            .as_array()
            .unwrap()
            .contains(&json!("--no-session"))
    );
    assert!(!directory.path().join("session.jsonl").exists());
    assert_eq!(details.models.len(), 5);
    for (index, (model, expected)) in details
        .models
        .iter()
        .zip([
            Some("medium"),
            Some("medium"),
            Some("high"),
            Some("low"),
            None,
        ])
        .enumerate()
    {
        assert_eq!(model["isDefault"], index == 0);
        assert_eq!(model["defaultThinkingOptionId"].as_str(), expected);
        if let Some(level) = expected {
            let mut selection = spec.clone();
            selection.config.model = model["id"].as_str().map(str::to_owned);
            selection.config.thinking_option_id = Some(level.to_owned());
            let mut session = client.create_session(&selection).await.unwrap();
            assert_eq!(
                native_thinking(&session.runtime_info().await.unwrap()),
                expected
            );
            session.close().await.unwrap();
        }
    }
}

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
async fn state_read_failures_keep_models_without_inventing_defaults() {
    for scenario in [
        "discovery-state-rejected",
        "discovery-state-timeout",
        "discovery-state-late",
        "discovery-state-exit",
        "discovery-state-malformed",
    ] {
        let (directory, mut client, spec) = fixture(scenario);
        client.deadline = Duration::from_secs(2);

        let details = client.discover(&spec.cwd).await.unwrap();
        assert_eq!(details.models[0]["id"], "fixture/model", "{scenario}");
        assert_eq!(details.models[0]["isSelectable"], true, "{scenario}");
        assert_eq!(details.models[0]["isDefault"], true, "{scenario}");
        assert!(details.models[0].get("defaultThinkingOptionId").is_none());
        assert!(
            !details.models[0]["thinkingOptions"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(!directory.path().join("session.jsonl").exists());
    }
}

#[tokio::test]
async fn successful_state_without_a_native_model_leaves_defaults_unspecified() {
    let (_directory, client, spec) = fixture("discovery-state-missing-model");

    let details = client.discover(&spec.cwd).await.unwrap();

    assert_eq!(details.models[0]["id"], "fixture/model");
    assert_eq!(details.models[0]["isDefault"], true);
    assert!(details.models[0].get("defaultThinkingOptionId").is_none());
}

#[tokio::test]
async fn selection_validation_reads_only_the_model_catalog() {
    for scenario in [
        "",
        "discovery-state-slow",
        "discovery-state-rejected",
        "discovery-state-timeout",
        "discovery-state-late",
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

#[tokio::test]
async fn model_preselection_is_independent_of_optional_state_and_native_model() {
    for scenario in ["", "discovery-state-late", "discovery-second-model"] {
        let (_directory, client, spec) = fixture(scenario);
        let details = client.discover(&spec.cwd).await.unwrap();
        let selected = details
            .models
            .iter()
            .find(|model| model["isDefault"] == true)
            .or_else(|| details.models.first())
            .unwrap();
        assert_eq!(selected["id"], "fixture/model", "{scenario}");
    }
}
