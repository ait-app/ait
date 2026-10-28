//! Next-turn controls retain intent across observation, settlement, and restart.

use super::*;

#[tokio::test]
async fn deferred_controls_distinguish_rejection_from_uncertain_transport_failure() {
    for (scenario, retained) in [
        ("reject-thinking-readback-exit", false),
        ("deferred-thinking-exit", true),
    ] {
        let (_directory, client, spec) = fixture(scenario);
        let mut session = client.create_session(&spec).await.unwrap();
        let turn = session.start_turn("wait", &spec.config).await.unwrap();
        let patch = ConfigPatch {
            thinking_option_id: NullableSetting::Set("high".into()),
            ..ConfigPatch::default()
        };
        let next = patch.apply(&spec.config);
        session.apply_config(&next, &patch).await.unwrap();
        session.cancel_turn(&turn).await.unwrap();
        terminal(session.as_mut()).await;

        let result = session.start_turn("hello", &next).await;

        assert!(
            matches!(
                result,
                Err(AgentSessionError::Rejected) if !retained
            ) || matches!(result, Err(AgentSessionError::Failed) if retained)
        );
        assert_eq!(
            session
                .persistence()
                .unwrap()
                .metadata
                .unwrap()
                .contains_key("deferredConfig"),
            retained
        );
        assert_eq!(
            session
                .effective_config()
                .unwrap()
                .thinking_option_id
                .is_some(),
            retained
        );
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejected_deferred_controls_preserve_partial_success_without_replaying_on_resume() {
    for apply_with_turn in [false, true] {
        for preference in [None, Some("medium")] {
            let (directory, client, mut spec) = fixture("reject-thinking-after-model");
            spec.config.thinking_option_id = preference.map(str::to_owned);
            let registry =
                FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json"));
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
            manager.send("agent", "wait").await.unwrap();
            manager
                .configure(
                    "agent",
                    &ConfigPatch {
                        model_id: NullableSetting::Set("fixture/other".into()),
                        thinking_option_id: NullableSetting::Set("high".into()),
                        ..ConfigPatch::default()
                    },
                )
                .await
                .unwrap();
            manager.cancel("agent").await.unwrap();
            settle_manager_turn(&mut manager).await;

            let result = if apply_with_turn {
                manager.send("agent", "hello").await
            } else {
                manager
                    .configure("agent", &ConfigPatch::default())
                    .await
                    .map(|_| ())
            };
            assert!(result.is_err());
            // Shutdown must retain the failed controls even without another poll.
            manager.close_all().await.unwrap();

            let record = registry.get("agent").unwrap().unwrap();
            let config = record.config.unwrap();
            assert_eq!(config.model.as_deref(), Some("fixture/other"));
            assert_eq!(
                config.thinking_option_id.as_deref(),
                preference.map(|_| "low")
            );
            let info = record.runtime_info.unwrap();
            assert_eq!(info.model.as_deref(), Some("fixture/other"));
            assert_eq!(info.thinking_option_id.as_deref(), Some("low"));
            assert!(!info.extra.unwrap().contains_key("configPending"));
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
            manager.resume("agent").await.unwrap();
            manager.send("agent", "hello").await.unwrap();
            settle_manager_turn(&mut manager).await;
            manager.close_all().await.unwrap();
            let rejected = std::fs::read_to_string(directory.path().join("requests.jsonl"))
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .filter(|command| {
                    command["type"] == "set_thinking_level" && command["level"] == "high"
                })
                .count();
            assert_eq!(rejected, 1);
        }
    }
}

#[tokio::test]
async fn deferred_model_switch_persists_dropped_thinking_before_polling() {
    let (directory, client, mut spec) = fixture("multiple-models");
    spec.config.model = Some("fixture/model".into());
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
    manager.send("agent", "wait").await.unwrap();
    assert_eq!(
        manager
            .configure(
                "agent",
                &ConfigPatch {
                    model_id: NullableSetting::Set("fixture/other".into()),
                    ..ConfigPatch::default()
                },
            )
            .await
            .unwrap(),
        ConfigurationOutcome::Deferred
    );

    // This is the durable crash-recovery snapshot immediately after acceptance.
    let record = registry.get("agent").unwrap().unwrap();
    let config = record.config.unwrap();
    assert_eq!(config.model.as_deref(), Some("fixture/other"));
    assert_eq!(config.thinking_option_id, None);
    let handle = record.persistence.unwrap();
    let pending: super::super::super::controls::DeferredConfig =
        serde_json::from_value(handle.metadata.unwrap()["deferredConfig"].clone()).unwrap();
    assert_eq!(pending.config, config);
    assert_eq!(
        record.runtime_info.unwrap().thinking_option_id.as_deref(),
        Some("high")
    );
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn deferred_resume_reconciles_models_without_resetting_an_already_selected_model() {
    for selected in [false, true] {
        for thinking in [
            NullableSetting::Unchanged,
            NullableSetting::Clear,
            NullableSetting::Set("high".into()),
        ] {
            let (directory, client, mut spec) = fixture("multiple-models");
            spec.config.model = Some("fixture/model".into());
            let mut session = client.create_session(&spec).await.unwrap();
            session.start_turn("wait", &spec.config).await.unwrap();
            let patch = ConfigPatch {
                model_id: NullableSetting::Set("fixture/other".into()),
                thinking_option_id: thinking.clone(),
                ..ConfigPatch::default()
            };
            session
                .apply_config(&patch.apply(&spec.config), &patch)
                .await
                .unwrap();
            spec.config = session.effective_config().unwrap();
            let handle = session.persistence().unwrap();
            session.close().await.unwrap();

            // Simulate native progress before a crash left the deferred handle uncleared.
            if selected {
                let native_path = directory.path().join("session.state.json");
                let mut native: Value =
                    serde_json::from_slice(&std::fs::read(&native_path).unwrap()).unwrap();
                native["model"]["id"] = json!("other");
                native["thinking"] = json!("xhigh");
                std::fs::write(native_path, serde_json::to_vec(&native).unwrap()).unwrap();
            }
            let requests = directory.path().join("requests.jsonl");
            let before = std::fs::read_to_string(&requests).unwrap();
            let mut resumed = client
                .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
                .await
                .unwrap();
            let info = resumed.runtime_info().await.unwrap();
            let expected_level = match thinking {
                NullableSetting::Unchanged if selected => "xhigh",
                NullableSetting::Unchanged => "low",
                NullableSetting::Clear => "medium",
                NullableSetting::Set(_) => "high",
            };
            assert_eq!(info.model.as_deref(), Some("fixture/other"));
            assert_eq!(native_thinking(&info), Some(expected_level));
            let after = std::fs::read_to_string(requests).unwrap();
            let commands: Vec<Value> = after[before.len()..]
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .filter(|command: &Value| {
                    matches!(
                        command["type"].as_str(),
                        Some("set_model" | "set_thinking_level")
                    )
                })
                .collect();
            let mut expected = Vec::new();
            if !selected {
                expected.push("set_model");
            }
            if thinking != NullableSetting::Unchanged {
                expected.push("set_thinking_level");
            }
            assert_eq!(
                commands
                    .iter()
                    .map(|command| command["type"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                expected
            );
            assert!(
                !resumed
                    .persistence()
                    .unwrap()
                    .metadata
                    .unwrap()
                    .contains_key("deferredConfig")
            );
            resumed.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn deferred_clear_survives_restart_and_executes_only_once() {
    let (directory, client, mut spec) = fixture("multiple-models");
    spec.config.model = Some("fixture/model".into());
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
    manager.send("agent", "wait").await.unwrap();
    assert_eq!(
        manager
            .configure(
                "agent",
                &ConfigPatch {
                    thinking_option_id: NullableSetting::Clear,
                    ..ConfigPatch::default()
                }
            )
            .await
            .unwrap(),
        ConfigurationOutcome::Deferred
    );
    manager.poll().await.unwrap();
    let record = registry.get("agent").unwrap().unwrap();
    assert_eq!(record.config.unwrap().thinking_option_id, None);
    let info = record.runtime_info.unwrap();
    assert_eq!(info.thinking_option_id.as_deref(), Some("high"));
    assert_eq!(info.extra.unwrap()["configPending"], true);
    manager.close_all().await.unwrap();
    let requests = directory.path().join("requests.jsonl");
    let before = std::fs::read_to_string(&requests).unwrap();
    for _ in 0..2 {
        manager = AgentManager::new(Box::new(registry.clone()));
        manager.register_client(Box::new(client.clone())).unwrap();
        let record = manager
            .restore("agent", &crate::protocol::resume::Overrides::default())
            .await
            .unwrap();
        assert_eq!(record.config.unwrap().thinking_option_id, None);
        assert_eq!(
            record.runtime_info.unwrap().thinking_option_id.as_deref(),
            Some("medium")
        );
        manager.close_all().await.unwrap();
    }
    let after = std::fs::read_to_string(requests).unwrap();
    let controls: Vec<Value> = after[before.len()..]
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .filter(|command: &Value| command["type"] == "set_thinking_level")
        .collect();
    assert_eq!(controls.len(), 1);
    assert_eq!(controls[0]["level"], "medium");
}

#[tokio::test]
async fn deferred_model_then_thinking_applies_before_the_next_prompt() {
    let (directory, client, spec) = fixture("multiple-models");
    let mut session = client.create_session(&spec).await.unwrap();
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    let model = ConfigPatch {
        model_id: NullableSetting::Set("fixture/other".into()),
        ..ConfigPatch::default()
    };
    let mut next = model.apply(&spec.config);
    session.apply_config(&next, &model).await.unwrap();
    let thinking = ConfigPatch {
        thinking_option_id: NullableSetting::Set("high".into()),
        ..ConfigPatch::default()
    };
    next = thinking.apply(&next);
    session.apply_config(&next, &thinking).await.unwrap();
    session.cancel_turn(&turn).await.unwrap();
    terminal(session.as_mut()).await;
    session.start_turn("hello", &next).await.unwrap();
    terminal(session.as_mut()).await;
    let info = session.runtime_info().await.unwrap();
    assert_eq!(info.model.as_deref(), Some("fixture/other"));
    assert_eq!(info.thinking_option_id.as_deref(), Some("high"));
    let commands: Vec<Value> = std::fs::read_to_string(directory.path().join("requests.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let model = commands
        .iter()
        .position(|command| command["type"] == "set_model")
        .unwrap();
    let thinking = commands
        .iter()
        .position(|command| command["type"] == "set_thinking_level")
        .unwrap();
    let last_prompt = commands
        .iter()
        .rposition(|command| command["type"] == "prompt")
        .unwrap();
    assert!(model < thinking && thinking < last_prompt);
    session.close().await.unwrap();
}

#[tokio::test]
async fn missing_native_model_does_not_erase_saved_selection_on_registration_or_resume() {
    let (directory, client, mut spec) = fixture("missing-model-state");
    spec.config.model = Some("fixture/model".into());
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
        created.config.unwrap().model.as_deref(),
        Some("fixture/model")
    );
    assert_eq!(created.runtime_info.unwrap().model, None);
    manager.close_all().await.unwrap();
    let record = manager
        .restore("agent", &crate::protocol::resume::Overrides::default())
        .await
        .unwrap();
    assert_eq!(
        record.config.unwrap().model.as_deref(),
        Some("fixture/model")
    );
    manager.close_all().await.unwrap();
}
