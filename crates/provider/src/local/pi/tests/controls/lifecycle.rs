//! Runtime observation and configuration must not extend or destroy native work.

use super::*;

/// Return whether the fixture's latest native writer is still alive.
pub(super) fn process_running(directory: &TempDir) -> bool {
    let pid = std::fs::read_to_string(directory.path().join("rpc.pid")).unwrap();
    std::process::Command::new("/bin/kill")
        .args(["-0", pid.trim()])
        .output()
        .unwrap()
        .status
        .success()
}

#[tokio::test]
async fn active_runtime_inspection_never_sends_a_state_request() {
    for scenario in ["active-state-rejected", "active-state-hang"] {
        let (directory, mut client, spec) = fixture(scenario);
        client.deadline = Duration::from_secs(1);
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
        manager.poll().await.unwrap();
        assert!(manager.active_turn("agent").is_some(), "{scenario}");
        assert!(registry.get("agent").unwrap().unwrap().last_error.is_none());
        let commands: Vec<Value> = std::fs::read_to_string(directory.path().join("requests.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let prompt = commands
            .iter()
            .position(|command| command["type"] == "prompt")
            .unwrap();
        assert!(
            commands[prompt + 1..]
                .iter()
                .all(|command| command["type"] != "get_state")
        );
        assert!(process_running(&directory));
        manager.cancel("agent").await.unwrap();
        manager.poll().await.unwrap();
        manager.send("agent", "hello").await.unwrap();
        manager.poll().await.unwrap();
        assert!(registry.get("agent").unwrap().unwrap().last_error.is_none());
        manager.close_all().await.unwrap();
    }
}

#[tokio::test]
async fn missing_thinking_readback_keeps_explicit_preference_without_resetting_native_level() {
    let (directory, client, mut spec) = fixture("thinking-state-disappears");
    spec.config.thinking_option_id = Some("high".into());
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
        Some("high")
    );
    for _ in 0..2 {
        manager.send("agent", "hello").await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while manager.active_turn("agent").is_some() {
                manager.poll().await.unwrap();
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
        let record = registry.get("agent").unwrap().unwrap();
        assert_eq!(
            record.config.unwrap().thinking_option_id.as_deref(),
            Some("high")
        );
        assert_eq!(record.runtime_info.unwrap().thinking_option_id, None);
    }
    let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    let levels: Vec<Value> = requests
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|command| command["type"] == "set_thinking_level")
        .map(|command| command["level"].clone())
        .collect();
    assert_eq!(levels, [json!("high")]);
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn dormant_controls_release_native_sessions_on_success_and_rejection() {
    for scenario in [
        "multiple-models",
        "reject-thinking",
        "reject-thinking-after-model",
    ] {
        let (directory, client, spec) = fixture(scenario);
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
        registry
            .update("agent", &|current| {
                let mut next = current.clone();
                next.last_status = domain::agent_runtime::AgentRuntimeStatus::Idle;
                next
            })
            .unwrap();
        let result = manager
            .configure(
                "agent",
                &ConfigPatch {
                    model_id: if scenario == "reject-thinking-after-model" {
                        NullableSetting::Set("fixture/other".into())
                    } else {
                        NullableSetting::Unchanged
                    },
                    thinking_option_id: NullableSetting::Set("high".into()),
                    ..ConfigPatch::default()
                },
            )
            .await;
        assert_eq!(result.is_ok(), scenario == "multiple-models");
        assert!(!manager.has_sessions(), "{scenario}");
        assert!(!process_running(&directory), "{scenario}");
        let record = registry.get("agent").unwrap().unwrap();
        assert_eq!(
            record.last_status,
            domain::agent_runtime::AgentRuntimeStatus::Idle
        );
        if scenario == "reject-thinking-after-model" {
            assert_eq!(
                record.runtime_info.unwrap().model.as_deref(),
                Some("fixture/other")
            );
        } else if result.is_ok() {
            assert_eq!(
                record.config.unwrap().thinking_option_id.as_deref(),
                Some("high")
            );
        }
    }
}

#[tokio::test]
async fn dormant_controls_preserve_untouched_selections_before_the_first_prompt() {
    for (thinking, expected) in [
        (NullableSetting::Unchanged, "xhigh"),
        (NullableSetting::Set("high".into()), "high"),
        (NullableSetting::Clear, "medium"),
    ] {
        let (directory, client, mut spec) = fixture("multiple-models");
        spec.config.model = Some("fixture/other".into());
        spec.config.thinking_option_id = Some("xhigh".into());
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
        let identity = created.persistence.unwrap().session_id;
        manager.close_all().await.unwrap();
        let requests = directory.path().join("requests.jsonl");
        std::fs::write(&requests, "").unwrap();

        manager
            .configure(
                "agent",
                &ConfigPatch {
                    thinking_option_id: thinking.clone(),
                    ..ConfigPatch::default()
                },
            )
            .await
            .unwrap();

        let record = registry.get("agent").unwrap().unwrap();
        assert_eq!(
            record.config.unwrap().model.as_deref(),
            Some("fixture/other")
        );
        let info = record.runtime_info.unwrap();
        assert_eq!(info.model.as_deref(), Some("fixture/other"));
        assert_eq!(info.thinking_option_id.as_deref(), Some(expected));
        assert_eq!(record.persistence.unwrap().session_id, identity);
        assert!(!manager.has_sessions());
        assert!(!process_running(&directory));
        let commands: Vec<Value> = std::fs::read_to_string(requests)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .filter(|command: &Value| command["type"] == "set_thinking_level")
            .collect();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0]["level"], info.thinking_option_id.unwrap());
    }
}

#[tokio::test]
async fn clearing_a_model_preserves_its_native_selection_live_and_dormant() {
    for dormant in [true, false] {
        for (thinking, expected) in [
            (NullableSetting::Unchanged, "xhigh"),
            (NullableSetting::Set("high".into()), "high"),
            (NullableSetting::Clear, "medium"),
        ] {
            let (directory, client, mut spec) = fixture("multiple-models");
            spec.config.model = Some("fixture/other".into());
            spec.config.thinking_option_id = Some("xhigh".into());
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
            if dormant {
                manager.close_all().await.unwrap();
            }
            let requests = directory.path().join("requests.jsonl");
            std::fs::write(&requests, "").unwrap();

            manager
                .configure(
                    "agent",
                    &ConfigPatch {
                        model_id: NullableSetting::Clear,
                        thinking_option_id: thinking.clone(),
                        ..ConfigPatch::default()
                    },
                )
                .await
                .unwrap();

            let record = registry.get("agent").unwrap().unwrap();
            let config = record.config.unwrap();
            assert_eq!(config.model.as_deref(), Some("fixture/other"));
            assert_eq!(
                config.thinking_option_id.as_deref(),
                (thinking != NullableSetting::Clear).then_some(expected)
            );
            let info = record.runtime_info.unwrap();
            assert_eq!(info.model.as_deref(), Some("fixture/other"));
            assert_eq!(info.thinking_option_id.as_deref(), Some(expected));
            let model_commands = std::fs::read_to_string(&requests)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .filter(|command| command["type"] == "set_model")
                .count();
            assert_eq!(model_commands, usize::from(dormant));
            manager.send("agent", "hello").await.unwrap();
            settle_manager_turn(&mut manager).await;
            manager.close_all().await.unwrap();
        }
    }
}

#[tokio::test]
async fn dormant_model_replacement_does_not_replay_an_obsolete_saved_model() {
    let (directory, client, mut spec) = fixture("multiple-models");
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
    registry
        .update("agent", &|current| {
            let mut next = current.clone();
            next.config.as_mut().unwrap().model = Some("fixture/removed".into());
            next
        })
        .unwrap();
    let identity = registry
        .get("agent")
        .unwrap()
        .unwrap()
        .persistence
        .unwrap()
        .session_id;
    let requests = directory.path().join("requests.jsonl");
    std::fs::write(&requests, "").unwrap();

    let outcome = manager
        .configure(
            "agent",
            &ConfigPatch {
                model_id: NullableSetting::Set("fixture/other".into()),
                ..ConfigPatch::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(outcome, ConfigurationOutcome::Applied);
    let record = registry.get("agent").unwrap().unwrap();
    assert_eq!(
        record.config.unwrap().model.as_deref(),
        Some("fixture/other")
    );
    let info = record.runtime_info.unwrap();
    assert_eq!(info.model.as_deref(), Some("fixture/other"));
    assert_eq!(info.thinking_option_id.as_deref(), Some("low"));
    assert_eq!(record.persistence.unwrap().session_id, identity);
    assert!(!manager.has_sessions());
    assert!(!process_running(&directory));
    let commands: Vec<Value> = std::fs::read_to_string(requests)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect();
    assert!(
        commands
            .iter()
            .all(|command| command["type"] != "set_thinking_level")
    );
    let models: Vec<_> = commands
        .iter()
        .filter(|command| command["type"] == "set_model")
        .collect();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0]["modelId"], "other");
}

#[tokio::test]
async fn setup_configuration_uses_memory_only_native_session() {
    let (directory, client, spec) = fixture("multiple-models");
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
    manager.close("agent").await.unwrap();
    manager
        .configure(
            "agent",
            &ConfigPatch {
                model_id: NullableSetting::Set("fixture/other".into()),
                ..ConfigPatch::default()
            },
        )
        .await
        .unwrap();
    let args: Vec<String> = serde_json::from_str(
        &std::fs::read_to_string(directory.path().join("arguments.json")).unwrap(),
    )
    .unwrap();
    assert!(args.iter().any(|arg| arg == "--no-session"));
    assert!(!directory.path().join("session.jsonl").exists());
    let configured = registry.get("agent").unwrap().unwrap();
    assert_eq!(
        configured.persistence.as_ref().unwrap().session_id,
        created.persistence.unwrap().session_id
    );
    assert_eq!(
        configured.config.unwrap().model.as_deref(),
        Some("fixture/other")
    );
    manager.send("agent", "hello").await.unwrap();
    manager.poll().await.unwrap();
    let args: Vec<String> = serde_json::from_str(
        &std::fs::read_to_string(directory.path().join("arguments.json")).unwrap(),
    )
    .unwrap();
    assert!(!args.iter().any(|arg| arg == "--no-session"));
    manager.close_all().await.unwrap();
}
