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
