//! Native observations must not manufacture a host thinking preference.

use super::*;

async fn complete_turn(manager: &mut AgentManager, text: &str) {
    manager.send("agent", text).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while manager.active_turn("agent").is_some() {
            manager.poll().await.unwrap();
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn setup_thinking_reset_survives_temporary_writers_until_the_first_prompt() {
    for (model, expected) in [("other", "medium"), ("clamped", "low")] {
        let (directory, client, mut spec) = fixture("multiple-models");
        spec.config.model = Some(format!("fixture/{model}"));
        spec.config.thinking_option_id = Some("high".into());
        let registry = FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json"));
        let mut manager = AgentManager::new(Box::new(registry.clone()));
        manager.register_client(Box::new(client.clone())).unwrap();
        manager
            .create_with_environment(
                "agent",
                &spec,
                AgentRegistration::default(),
                &client.environment,
            )
            .await
            .unwrap();
        manager.close_all().await.unwrap();
        manager
            .configure(
                "agent",
                &ConfigPatch {
                    thinking_option_id: NullableSetting::Clear,
                    ..ConfigPatch::default()
                },
            )
            .await
            .unwrap();
        assert!(!manager.has_sessions());
        let configured = registry.get("agent").unwrap().unwrap();
        assert_eq!(configured.config.unwrap().thinking_option_id, None);
        assert_eq!(
            configured
                .runtime_info
                .unwrap()
                .thinking_option_id
                .as_deref(),
            Some(expected)
        );

        // Opening and closing another setup-only writer must not consume the reset.
        for _ in 0..2 {
            manager = AgentManager::new(Box::new(registry.clone()));
            manager.register_client(Box::new(client.clone())).unwrap();
            manager.resume("agent").await.unwrap();
            manager.close_all().await.unwrap();
        }
        manager = AgentManager::new(Box::new(registry.clone()));
        manager.register_client(Box::new(client.clone())).unwrap();
        complete_turn(&mut manager, "hello").await;

        let native_state_path = directory.path().join("session.state.json");
        let mut native_state: Value =
            serde_json::from_str(&std::fs::read_to_string(&native_state_path).unwrap()).unwrap();
        assert_eq!(native_state["thinking"], expected, "{model}");
        let record = registry.get("agent").unwrap().unwrap();
        assert_eq!(record.config.unwrap().thinking_option_id, None);
        assert!(
            !record
                .persistence
                .unwrap()
                .metadata
                .unwrap()
                .contains_key("deferredConfig")
        );
        manager.close_all().await.unwrap();

        // Once history exists, independent native changes remain authoritative.
        native_state["thinking"] = json!("ultra");
        std::fs::write(
            &native_state_path,
            serde_json::to_vec(&native_state).unwrap(),
        )
        .unwrap();
        manager = AgentManager::new(Box::new(registry.clone()));
        manager.register_client(Box::new(client)).unwrap();
        complete_turn(&mut manager, "hello").await;
        let record = registry.get("agent").unwrap().unwrap();
        assert_eq!(
            record.runtime_info.unwrap().thinking_option_id.as_deref(),
            Some("ultra")
        );
        assert_eq!(record.config.unwrap().thinking_option_id, None);
        manager.close_all().await.unwrap();
    }
}

#[tokio::test]
async fn only_successful_controls_supersede_a_dormant_setup_reset() {
    let model = ConfigPatch {
        model_id: NullableSetting::Set("fixture/other".into()),
        ..ConfigPatch::default()
    };
    let thinking = ConfigPatch {
        thinking_option_id: NullableSetting::Set("high".into()),
        ..ConfigPatch::default()
    };
    let rejected_model = ConfigPatch {
        model_id: NullableSetting::Set("fixture/model".into()),
        ..ConfigPatch::default()
    };
    for (scenario, patch, expected, applied) in [
        ("multiple-models", &model, "low", true),
        ("multiple-models", &thinking, "high", true),
        ("reject-thinking-after-model", &thinking, "medium", false),
        ("reject-default-model", &rejected_model, "medium", false),
    ] {
        let (directory, client, mut spec) = fixture(scenario);
        spec.config.model = Some("fixture/other".into());
        let registry = FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json"));
        let mut manager = AgentManager::new(Box::new(registry.clone()));
        manager.register_client(Box::new(client.clone())).unwrap();
        manager
            .create_with_environment(
                "agent",
                &spec,
                AgentRegistration::default(),
                &client.environment,
            )
            .await
            .unwrap();
        manager.close_all().await.unwrap();
        manager
            .configure(
                "agent",
                &ConfigPatch {
                    thinking_option_id: NullableSetting::Clear,
                    ..ConfigPatch::default()
                },
            )
            .await
            .unwrap();

        assert_eq!(manager.configure("agent", patch).await.is_ok(), applied);

        let record = registry.get("agent").unwrap().unwrap();
        assert_eq!(
            record
                .persistence
                .unwrap()
                .metadata
                .unwrap()
                .contains_key("deferredConfig"),
            !applied
        );
        manager = AgentManager::new(Box::new(registry.clone()));
        manager.register_client(Box::new(client)).unwrap();
        complete_turn(&mut manager, "hello").await;
        let state: Value = serde_json::from_str(
            &std::fs::read_to_string(directory.path().join("session.state.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(state["thinking"], expected);
        manager.close_all().await.unwrap();
    }
}

#[tokio::test]
async fn rejected_setup_reset_replay_is_consumed() {
    let (directory, client, spec) = fixture("");
    let registry = FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json"));
    let mut manager = AgentManager::new(Box::new(registry.clone()));
    manager.register_client(Box::new(client.clone())).unwrap();
    manager
        .create_with_environment(
            "agent",
            &spec,
            AgentRegistration::default(),
            &client.environment,
        )
        .await
        .unwrap();
    manager
        .configure(
            "agent",
            &ConfigPatch {
                thinking_option_id: NullableSetting::Clear,
                ..ConfigPatch::default()
            },
        )
        .await
        .unwrap();
    manager.close_all().await.unwrap();
    let mut rejecting = client.clone();
    rejecting.environment = AgentEnvironment::try_from(BTreeMap::from([(
        "PI_FIXTURE_SCENARIO".into(),
        "reject-thinking".into(),
    )]))
    .unwrap();
    manager = AgentManager::new(Box::new(registry.clone()));
    manager.register_client(Box::new(rejecting)).unwrap();

    assert!(manager.resume("agent").await.is_err());

    assert!(!manager.has_sessions());
    let record = registry.get("agent").unwrap().unwrap();
    assert_eq!(record.config.unwrap().thinking_option_id, None);
    assert!(
        !record
            .persistence
            .unwrap()
            .metadata
            .unwrap()
            .contains_key("deferredConfig")
    );
    manager = AgentManager::new(Box::new(registry));
    manager.register_client(Box::new(client)).unwrap();
    complete_turn(&mut manager, "hello").await;
    manager.close_all().await.unwrap();
    let resets = std::fs::read_to_string(directory.path().join("requests.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|command| command["type"] == "set_thinking_level")
        .count();
    assert_eq!(resets, 2);
}

#[tokio::test]
async fn unspecified_and_cleared_preferences_survive_observation_and_repeated_resume() {
    for initial in [None, Some("high")] {
        let (directory, client, mut spec) = fixture("multiple-models");
        spec.config.thinking_option_id = initial.map(str::to_owned);
        let registry = FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json"));
        let mut manager = AgentManager::new(Box::new(registry.clone()));
        manager.register_client(Box::new(client.clone())).unwrap();
        let created = manager
            .create_with_environment(
                "agent",
                &spec,
                AgentRegistration::default(),
                &client.environment,
            )
            .await
            .unwrap();
        assert_eq!(
            created.config.unwrap().thinking_option_id.as_deref(),
            initial
        );
        if initial.is_some() {
            manager
                .configure(
                    "agent",
                    &ConfigPatch {
                        thinking_option_id: NullableSetting::Clear,
                        ..ConfigPatch::default()
                    },
                )
                .await
                .unwrap();
        }
        manager.poll().await.unwrap();
        let record = registry.get("agent").unwrap().unwrap();
        assert_eq!(record.config.unwrap().thinking_option_id, None);
        assert_eq!(
            record.runtime_info.unwrap().thinking_option_id.as_deref(),
            Some("medium")
        );

        // A model's default and an extension notification are facts, not preferences.
        manager
            .configure(
                "agent",
                &ConfigPatch {
                    model_id: NullableSetting::Set("fixture/model".into()),
                    ..ConfigPatch::default()
                },
            )
            .await
            .unwrap();
        complete_turn(&mut manager, "thinking-after").await;
        complete_turn(&mut manager, "hello").await;
        let record = registry.get("agent").unwrap().unwrap();
        assert_eq!(record.config.unwrap().thinking_option_id, None);
        assert_eq!(
            record.runtime_info.unwrap().thinking_option_id.as_deref(),
            Some("low")
        );
        let identity = record.persistence.unwrap().session_id;
        manager.close_all().await.unwrap();

        for native_level in ["ultra", "minimal"] {
            // Pi changes independently while the host is disconnected.
            let native_state_path = directory.path().join("session.state.json");
            let mut state: Value =
                serde_json::from_str(&std::fs::read_to_string(&native_state_path).unwrap())
                    .unwrap();
            state["thinking"] = json!(native_level);
            std::fs::write(&native_state_path, serde_json::to_vec(&state).unwrap()).unwrap();
            let requests = directory.path().join("requests.jsonl");
            let before = std::fs::read_to_string(&requests).unwrap();

            manager = AgentManager::new(Box::new(registry.clone()));
            manager.register_client(Box::new(client.clone())).unwrap();
            let restored = manager
                .restore("agent", &crate::protocol::resume::Overrides::default())
                .await
                .unwrap();
            assert_eq!(restored.config.unwrap().thinking_option_id, None);
            assert_eq!(
                restored.runtime_info.unwrap().thinking_option_id.as_deref(),
                Some(native_level)
            );
            assert_eq!(restored.persistence.unwrap().session_id, identity);
            complete_turn(&mut manager, "hello").await;
            manager.close_all().await.unwrap();
            let after = std::fs::read_to_string(&requests).unwrap();
            assert!(after[before.len()..].lines().all(|line| {
                serde_json::from_str::<Value>(line).unwrap()["type"] != "set_thinking_level"
            }));
            let record = registry.get("agent").unwrap().unwrap();
            assert_eq!(record.config.unwrap().thinking_option_id, None);
            assert_eq!(
                record.runtime_info.unwrap().thinking_option_id.as_deref(),
                Some(native_level)
            );
        }
    }
}
