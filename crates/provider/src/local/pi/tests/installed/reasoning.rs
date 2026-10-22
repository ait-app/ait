use super::*;

#[tokio::test]
#[ignore = "requires Pi >= 1.2 on PATH; inspects model requests on an isolated loopback server"]
async fn installed_pi_reasoning_is_sent_to_model_api() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let (address, requests, _server) = loopback_model().await;
    let client = reasoning_client(&root, &format!("http://{address}/v1"));
    let mut spec = AgentSessionSpec {
        provider: PROVIDER.into(),
        cwd: root.to_str().unwrap().into(),
        config: StoredAgentConfig::default(),
    };
    let mut session = client.create_session(&spec).await.unwrap();

    for (selection, expected_effort) in [
        ("low", Some("low")),
        ("high", Some("high")),
        ("xhigh", Some("high")),
        ("off", None),
    ] {
        spec.config.thinking_option_id = Some(selection.into());
        client.validate_selection(&spec).await.unwrap();
        let previous_requests = requests.lock().unwrap().len();
        session.start_turn("Say hello", &spec.config).await.unwrap();
        let events = terminal(session.as_mut()).await;
        assert!(
            matches!(events.last(), Some(AgentTurnEvent::Completed(Some(text))) if text == "Hello from real Pi")
        );
        assert_eq!(
            native_thinking(&session.runtime_info().await.unwrap()),
            Some(selection)
        );

        let observed = requests.lock().unwrap();
        let turn_requests = &observed[previous_requests..];
        assert!(!turn_requests.is_empty(), "{selection}: no model request");
        for request in turn_requests {
            assert_eq!(request["model"], "reasoner");
            assert_eq!(
                request["reasoning_effort"].as_str(),
                expected_effort,
                "{selection}: outgoing reasoning effort did not match Pi's native mapping"
            );
            if expected_effort.is_none() {
                assert!(request.get("reasoning_effort").is_none());
            }
        }
    }
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires Pi >= 1.2 on PATH; uses only an isolated loopback model"]
async fn installed_pi_model_changes_preserve_effective_thinking() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let (address, _requests, _server) = loopback_model().await;
    let client = reasoning_client(&root, &format!("http://{address}/v1"));
    let models_path = root.join("native/models.json");
    let mut models: Value = serde_json::from_slice(&std::fs::read(&models_path).unwrap()).unwrap();
    let catalog = models["providers"]["ait-fixture"]["models"]
        .as_array_mut()
        .unwrap();
    let mut other = catalog[0].clone();
    other["id"] = json!("other");
    catalog.push(other);
    std::fs::write(models_path, models.to_string()).unwrap();
    let mut spec = AgentSessionSpec {
        provider: PROVIDER.into(),
        cwd: root.to_str().unwrap().into(),
        config: StoredAgentConfig {
            model: Some("ait-fixture/reasoner".into()),
            thinking_option_id: Some("high".into()),
            ..StoredAgentConfig::default()
        },
    };
    let mut session = client.create_session(&spec).await.unwrap();
    spec.config.model = Some("ait-fixture/other".into());
    session.start_turn("Say hello", &spec.config).await.unwrap();
    terminal(session.as_mut()).await;
    assert_eq!(
        native_thinking(&session.runtime_info().await.unwrap()),
        Some("high")
    );
    session
        .start_turn("/ait_think", &spec.config)
        .await
        .unwrap();
    terminal(session.as_mut()).await;
    spec.config.model = Some("ait-fixture/reasoner".into());
    session.start_turn("Say hello", &spec.config).await.unwrap();
    terminal(session.as_mut()).await;
    assert_eq!(
        native_thinking(&session.runtime_info().await.unwrap()),
        Some("minimal")
    );
    session.close().await.unwrap();
}
