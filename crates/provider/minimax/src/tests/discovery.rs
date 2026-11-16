use super::*;

#[tokio::test]
async fn advertised_but_rejected_models_cannot_be_selected_for_creation() {
    let (root, client, mut spec) = fixture();
    std::fs::write(root.path().join("scenario"), "rejected-model").unwrap();
    spec.config.model = Some("minimax/rejected".into());
    assert!(matches!(
        client.create_session(&spec).await,
        Err(AgentSessionError::Rejected)
    ));

    let details = client.discover(&spec.cwd).await.unwrap();
    let rejected = details
        .models
        .iter()
        .find(|model| model["id"] == "minimax/rejected")
        .unwrap();
    assert_eq!(rejected["isSelectable"], false);
    assert_eq!(rejected["isDefault"], false);
    assert_eq!(details.models[1]["isDefault"], true);
    assert_eq!(details.models[2]["isDefault"], false);
    for model in details
        .models
        .iter()
        .filter(|model| model["isSelectable"] == true)
    {
        spec.config.model = Some(model["id"].as_str().unwrap().into());
        spec.config.thinking_option_id =
            model["defaultThinkingOptionId"].as_str().map(str::to_owned);
        let mut session = client.create_session(&spec).await.unwrap();
        assert_eq!(
            session.runtime_info().await.unwrap().model,
            spec.config.model
        );
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn discovery_reads_thinking_options_from_each_confirmed_model() {
    let (root, client, spec) = fixture();
    std::fs::write(root.path().join("scenario"), "alternate-effort").unwrap();
    let details = client.discover(&spec.cwd).await.unwrap();
    assert_eq!(details.models[0]["isDefault"], true);
    assert_eq!(details.models[0]["defaultThinkingOptionId"], "default");
    assert_eq!(details.models[1]["isDefault"], false);
    assert_eq!(details.models[1]["defaultThinkingOptionId"], "high");
    assert_eq!(
        details.models[1]["thinkingOptions"],
        json!([{"id":"high","label":"High","isDefault":true}])
    );
}

#[tokio::test]
async fn discovery_does_not_hide_native_transport_failures_as_unselectable_models() {
    let (root, client, spec) = fixture();
    std::fs::write(root.path().join("scenario"), "catalog-disconnected").unwrap();
    assert!(matches!(
        client.discover(&spec.cwd).await,
        Err(AgentSessionError::Failed)
    ));
}

#[tokio::test]
async fn discovery_drains_configuration_notifications_without_running_prompts() {
    let (root, client, spec) = fixture();
    std::fs::write(root.path().join("scenario"), "large-catalog").unwrap();
    let details = client.discover(&spec.cwd).await.unwrap();
    assert_eq!(details.models.len(), 142);
    assert!(
        details
            .models
            .iter()
            .all(|model| model["isSelectable"] == true)
    );
    let requests = std::fs::read_to_string(root.path().join("requests.jsonl")).unwrap();
    let requests: Vec<Value> = requests
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(requests.iter().all(|request| matches!(
        request["method"].as_str(),
        Some("initialize" | "session/new" | "session/set_config_option" | "session/close")
    )));
    assert_eq!(requests.last().unwrap()["method"], "session/close");
    assert!(
        requests
            .iter()
            .filter(|request| request["method"] == "session/set_config_option")
            .all(|request| request["params"]["configId"] == "model")
    );
}

#[tokio::test]
async fn discovery_bounds_the_entire_catalog_and_closes_the_native_session() {
    let (root, mut client, spec) = fixture();
    std::fs::write(root.path().join("scenario"), "slow-catalog").unwrap();
    client.deadline = Duration::from_millis(500);
    assert!(matches!(
        client.discover(&spec.cwd).await,
        Err(AgentSessionError::Failed)
    ));
    let requests = std::fs::read_to_string(root.path().join("requests.jsonl")).unwrap();
    let last: Value = serde_json::from_str(requests.lines().last().unwrap()).unwrap();
    assert_eq!(last["method"], "session/close");
}
