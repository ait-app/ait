use super::*;

#[tokio::test]
async fn each_model_keeps_its_native_thinking_options() {
    let (_root, client, spec) = fixture();
    let details = client.discover(&spec.cwd).await.unwrap();
    assert_eq!(
        details.models[0]["thinkingOptions"],
        json!([{"id":"off","label":"off"},{"id":"low","label":"low"},
            {"id":"high","label":"high"}])
    );
    assert_eq!(
        details.models[1]["thinkingOptions"],
        json!([{"id":"off","label":"off"},{"id":"low","label":"low"},
            {"id":"high","label":"high"},{"id":"max","label":"max"}])
    );
}

#[tokio::test]
async fn draft_tiers_follow_the_exact_model_without_persisting_native_settings() {
    let (root, client, mut spec) = fixture();
    for (provider, expected) in [("other", 2), ("test", 4), ("other", 2)] {
        spec.config.model = Some(json!([provider, "model"]).to_string());
        spec.config.feature_values = Some(BTreeMap::from([("fastMode".to_owned(), json!(true))]));
        let features = client.draft_features(&spec).await.unwrap();
        assert_eq!(features.len(), expected);
        assert_eq!(features[0]["id"], "autoCompaction");
        assert_eq!(features[1]["id"], "autoRetry");
        if provider == "test" {
            assert_eq!(features[2]["id"], "fastMode");
            assert_eq!(features[2]["value"], true);
            assert_eq!(features[3]["id"], "slowMode");
        }
    }
    spec.config.model = Some(json!(["unknown", "model"]).to_string());
    assert!(client.draft_features(&spec).await.is_err());
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    for kind in ["prompt", "set_model", "set_thinking_level", "set_slow_mode"] {
        assert!(!calls.contains(&format!("\"type\": \"{kind}\"")));
    }
    assert!(!root.path().join("agent/sessions").exists());
}

#[tokio::test]
async fn live_controls_do_not_offer_tiers_for_an_unsupported_model() {
    let (_root, client, mut spec) = fixture();
    for (provider, expected) in [("other", 2), ("test", 4)] {
        spec.config.model = Some(json!([provider, "model"]).to_string());
        let mut session = client.create_session(&spec).await.unwrap();
        let controls = session.control_settings(&spec.config).unwrap();
        assert_eq!(controls["features"].as_array().unwrap().len(), expected);
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn live_tiers_are_refreshed_after_model_changes_and_never_reused_for_a_pending_model() {
    let (_root, client, mut spec) = fixture();
    spec.config.model = Some(json!(["test", "model"]).to_string());
    let mut session = client.create_session(&spec).await.unwrap();
    for (provider, expected) in [("other", 2), ("test", 4)] {
        spec.config.model = Some(json!([provider, "model"]).to_string());
        let pending = session.control_settings(&spec.config).unwrap();
        assert_eq!(pending["features"].as_array().unwrap().len(), 2);
        session.start_turn("hello", &spec.config).await.unwrap();
        complete(session.as_mut()).await;
        let applied = session.control_settings(&spec.config).unwrap();
        assert_eq!(applied["features"].as_array().unwrap().len(), expected);
    }
    session.close().await.unwrap();
}
