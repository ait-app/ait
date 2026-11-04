use super::*;

#[tokio::test]
#[ignore = "requires Pi >= 1.2 on PATH; uses only an isolated loopback model"]
async fn installed_pi_off_catalog_hint_does_not_gate_native_sessions() {
    for mapped in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let (address, requests, _server) = loopback_model().await;
        let native = root.join("native");
        std::fs::create_dir(&native).unwrap();
        let mut model = json!({"id":"fixture-model","reasoning":true,"input":["text"],
            "contextWindow":8192,"maxTokens":128});
        if mapped {
            model["thinkingLevelMap"] = json!({"off":"none"});
        }
        std::fs::write(
            native.join("models.json"),
            json!({"providers":{"ait-fixture":{
                "baseUrl":format!("http://{address}/v1"),"api":"openai-completions",
                "apiKey":"fixture-only","models":[model]
            }}})
            .to_string(),
        )
        .unwrap();
        let mut client = PiClient::new("pi".into());
        client.environment = AgentEnvironment::try_from(BTreeMap::from([
            (
                "PI_CODING_AGENT_DIR".into(),
                native.to_str().unwrap().into(),
            ),
            ("HOME".into(), root.to_str().unwrap().into()),
        ]))
        .unwrap();
        client.session_environment = history::SessionEnvironment::isolated(&root, &native);
        let spec = AgentSessionSpec {
            provider: PROVIDER.into(),
            cwd: root.to_str().unwrap().into(),
            config: StoredAgentConfig {
                model: Some("ait-fixture/fixture-model".into()),
                thinking_option_id: Some("off".into()),
                ..StoredAgentConfig::default()
            },
        };
        let details = client.discover(&spec.cwd).await.unwrap();
        let model = details
            .models
            .iter()
            .find(|model| model["id"] == "ait-fixture/fixture-model")
            .unwrap();
        assert_eq!(
            model["thinkingOptions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|option| option["id"] == "off"),
            mapped
        );
        let mut session = fresh_resumed_session(&client, &spec).await;
        assert_eq!(
            native_thinking(&session.runtime_info().await.unwrap()),
            Some("off")
        );
        session
            .start_turn("Exercise the tool cards", &spec.config)
            .await
            .unwrap();
        assert!(matches!(
            terminal(session.as_mut()).await.last(),
            Some(AgentTurnEvent::Completed(_))
        ));
        let handle = session.persistence().unwrap();
        session.close().await.unwrap();
        let recorded =
            std::fs::read_to_string(handle.native_handle.as_ref().unwrap().as_str().unwrap())
                .unwrap();
        assert!(
            recorded
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .any(|record| record["type"] == "thinking_level_change"
                    && record["thinkingLevel"] == "off")
        );
        let imported = client.inspect_session(&handle, &spec.cwd).await.unwrap();
        assert_eq!(imported.config.thinking_option_id.as_deref(), Some("off"));
        let requests = requests.lock().unwrap();
        assert!(!requests.is_empty());
        if mapped {
            assert!(
                requests
                    .iter()
                    .all(|request| request["reasoning_effort"] == "none")
            );
        }
    }
}
