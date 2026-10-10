use super::*;

#[test]
fn catalog_speeds_deduplicate_and_accept_new_models_and_tiers() {
    let catalog = options(&json!({"serviceTiers":[
        {"id":"priority","name":"Fast"},{"id":"ultrafast","name":"Ultrafast"},
        {"id":"future","name":"Future speed"},{"id":"bad id"}],
        "additionalSpeedTiers":["priority","ultrafast"]}));
    assert_eq!(catalog.len(), 4);
    let config =
        serde_json::from_value(json!({"featureValues":{"service_tier":"ultrafast"}})).unwrap();
    assert!(validate(&config, &catalog).is_ok());
    let selector = feature(&config, &catalog).unwrap();
    assert_eq!(selector["value"], "ultrafast");
    assert_eq!(selector["desktopTrigger"], "icon");
    assert_eq!(selector["options"][3]["label"], "Future speed");
    assert!(feature(&StoredAgentConfig::default(), &options(&json!({}))).is_none());
}

#[test]
fn normal_overrides_thread_tier_and_legacy_fast_migrates_without_guessing_support() {
    let config: StoredAgentConfig = serde_json::from_value(
        json!({"featureValues":{"fast_mode":true,"service_tier":"default"}}),
    )
    .unwrap();
    assert_eq!(selected(&config), "default");
    let legacy: StoredAgentConfig =
        serde_json::from_value(json!({"featureValues":{"fast_mode":true}})).unwrap();
    let catalog = options(&json!({"serviceTiers":[{"id":"priority","name":"Fast"}]}));
    assert!(validate(&legacy, &catalog).is_ok());
    assert_eq!(feature(&legacy, &catalog).unwrap()["value"], "priority");
    let future_only = options(&json!({"additionalSpeedTiers":["future"]}));
    assert_eq!(
        validate(&legacy, &future_only),
        Err(AgentSessionError::Rejected)
    );
    assert_eq!(feature(&legacy, &future_only).unwrap()["value"], "default");
    assert_eq!(
        validate(&legacy, &options(&json!({}))),
        Err(AgentSessionError::Rejected)
    );
    let unavailable =
        serde_json::from_value(json!({"featureValues":{"service_tier":"ultrafast"}})).unwrap();
    assert_eq!(
        validate(&unavailable, &catalog),
        Err(AgentSessionError::Rejected)
    );
    assert_eq!(feature(&unavailable, &catalog).unwrap()["value"], "default");
}

#[cfg(unix)]
#[tokio::test]
async fn catalog_tier_reaches_turn_start_and_normal_explicitly_clears_it() {
    use crate::ports::agent_session::AgentClient;
    let fixture = crate::test_support::Fixture::new();
    std::fs::write(
        fixture.cwd.join("speed-models.json"),
        json!({"data":[{
        "model":"future-model","displayName":"Future model","isDefault":true,
        "supportedReasoningEfforts":[],"serviceTiers":[{"id":"priority","name":"Fast"},
        {"id":"ultrafast","name":"Ultrafast"},{"id":"future-tier","name":"Future speed"}]}],
        "nextCursor":null})
        .to_string(),
    )
    .unwrap();
    let client = fixture.client();
    let mut spec = fixture.spec();
    spec.config.model = Some("future-model".into());
    spec.config.feature_values = Some(std::collections::BTreeMap::from([(
        "service_tier".into(),
        json!("ultrafast"),
    )]));
    let features = client.draft_features(&spec).await.unwrap();
    assert_eq!(features[0]["options"].as_array().unwrap().len(), 4);
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("hang", &spec.config).await.unwrap();
    let first = fixture
        .requests()
        .into_iter()
        .find(|request| request["method"] == "turn/start")
        .unwrap();
    assert_eq!(first["params"]["serviceTier"], "ultrafast");
    session.cancel_turn("turn-1").await.ok();
    session.close().await.unwrap();
    spec.config
        .feature_values
        .as_mut()
        .unwrap()
        .insert("service_tier".into(), json!("default"));
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("normal", &spec.config).await.unwrap();
    let last = fixture
        .requests()
        .into_iter()
        .rev()
        .find(|request| request["method"] == "turn/start")
        .unwrap();
    assert_eq!(last["params"]["serviceTier"], "default");
    session.close().await.unwrap();
    spec.config.feature_values = Some(std::collections::BTreeMap::from([(
        "fast_mode".into(),
        json!(true),
    )]));
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("legacy fast", &spec.config)
        .await
        .unwrap();
    let legacy = fixture
        .requests()
        .into_iter()
        .rev()
        .find(|request| request["method"] == "turn/start")
        .unwrap();
    assert_eq!(legacy["params"]["serviceTier"], "priority");
    session.close().await.unwrap();
    spec.config
        .feature_values
        .as_mut()
        .unwrap()
        .insert("service_tier".into(), json!("unsupported"));
    assert!(client.validate_selection(&spec).await.is_err());
}
