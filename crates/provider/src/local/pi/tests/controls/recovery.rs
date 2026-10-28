//! Recovery reconciles accepted controls without losing intent or replaying rejections.

use super::*;
use crate::service::agent_manager::AgentManagerError;

#[tokio::test]
async fn deferred_resume_consumes_rejection_even_when_readback_fails() {
    for (scenario, retained) in [
        ("reject-thinking-readback-exit", false),
        ("deferred-thinking-exit", true),
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
        manager.send("agent", "wait").await.unwrap();
        manager
            .configure(
                "agent",
                &ConfigPatch {
                    thinking_option_id: NullableSetting::Set("high".into()),
                    ..ConfigPatch::default()
                },
            )
            .await
            .unwrap();
        manager.close_all().await.unwrap();
        manager = AgentManager::new(Box::new(registry.clone()));
        manager.register_client(Box::new(client)).unwrap();

        let error = manager.resume("agent").await.unwrap_err();

        assert_eq!(
            error,
            if retained {
                AgentManagerError::Session
            } else {
                AgentManagerError::SessionRejected
            }
        );
        let record = registry.get("agent").unwrap().unwrap();
        assert_eq!(
            record
                .persistence
                .unwrap()
                .metadata
                .unwrap()
                .contains_key("deferredConfig"),
            retained
        );
        assert_eq!(
            record.config.unwrap().thinking_option_id.is_some(),
            retained
        );
        assert!(!manager.has_sessions());
    }
}

#[tokio::test]
async fn rejected_deferred_resume_persists_consumption_and_partial_success() {
    for switch_model in [false, true] {
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
                        model_id: if switch_model {
                            NullableSetting::Set("fixture/other".into())
                        } else {
                            NullableSetting::Unchanged
                        },
                        thinking_option_id: NullableSetting::Set("high".into()),
                        ..ConfigPatch::default()
                    },
                )
                .await
                .unwrap();
            manager.close_all().await.unwrap();

            manager = AgentManager::new(Box::new(registry.clone()));
            manager.register_client(Box::new(client.clone())).unwrap();
            assert_eq!(
                manager.resume("agent").await.unwrap_err(),
                AgentManagerError::SessionRejected
            );
            let record = registry.get("agent").unwrap().unwrap();
            let expected_model = if switch_model {
                "fixture/other"
            } else {
                "fixture/model"
            };
            let expected_thinking = if switch_model { "low" } else { "medium" };
            let config = record.config.unwrap();
            assert_eq!(config.model.as_deref(), Some(expected_model));
            assert_eq!(
                config.thinking_option_id.as_deref(),
                preference.map(|_| expected_thinking)
            );
            let info = record.runtime_info.unwrap();
            assert_eq!(info.model.as_deref(), Some(expected_model));
            assert_eq!(info.thinking_option_id.as_deref(), Some(expected_thinking));
            assert!(!info.extra.unwrap().contains_key("configPending"));
            assert!(
                !record
                    .persistence
                    .unwrap()
                    .metadata
                    .unwrap()
                    .contains_key("deferredConfig")
            );
            assert!(!manager.has_sessions());

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
async fn renamed_history_preserves_a_deferred_clear_during_resume() {
    let (directory, client, mut spec) = fixture("multiple-models");
    spec.config.thinking_option_id = Some("high".into());
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("wait", &spec.config).await.unwrap();
    let patch = ConfigPatch {
        thinking_option_id: NullableSetting::Clear,
        ..ConfigPatch::default()
    };
    spec.config = patch.apply(&spec.config);
    session.apply_config(&spec.config, &patch).await.unwrap();
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let store = history::session_directory(&client, std::path::Path::new(&spec.cwd)).unwrap();
    std::fs::create_dir_all(&store).unwrap();
    let renamed = store.join("renamed.jsonl");
    std::fs::rename(directory.path().join("session.jsonl"), &renamed).unwrap();
    // The RPC fixture records only identity; history repair needs a native v3 header.
    std::fs::write(
        &renamed,
        format!(
            "{}\n",
            json!({
                "type":"session", "version":3, "id":handle.session_id,
                "cwd":spec.cwd, "timestamp":"2026-10-27T00:00:00Z"
            })
        ),
    )
    .unwrap();
    std::fs::rename(
        directory.path().join("session.state.json"),
        renamed.with_extension("state.json"),
    )
    .unwrap();
    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert_eq!(
        resumed
            .runtime_info()
            .await
            .unwrap()
            .thinking_option_id
            .as_deref(),
        Some("medium")
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

#[tokio::test]
async fn dormant_rejection_preserves_an_existing_thinking_preference() {
    let (directory, client, mut spec) = fixture("reject-thinking-after-model");
    spec.config.thinking_option_id = Some("low".into());
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
    assert!(
        manager
            .configure(
                "agent",
                &ConfigPatch {
                    thinking_option_id: NullableSetting::Set("high".into()),
                    ..ConfigPatch::default()
                }
            )
            .await
            .is_err()
    );
    let record = registry.get("agent").unwrap().unwrap();
    assert_eq!(
        record.config.unwrap().thinking_option_id.as_deref(),
        Some("low")
    );
    assert_eq!(
        record.runtime_info.unwrap().thinking_option_id.as_deref(),
        Some("medium")
    );
    assert!(!manager.has_sessions());
    manager = AgentManager::new(Box::new(registry));
    manager.register_client(Box::new(client)).unwrap();
    let resumed = manager.resume("agent").await.unwrap();
    assert_eq!(
        resumed.config.unwrap().thinking_option_id.as_deref(),
        Some("low")
    );
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn shutdown_after_live_deferred_rejection_retains_consumption_without_readback() {
    for apply_with_turn in [true, false] {
        let (directory, client, spec) = fixture("reject-thinking-readback-exit");
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
        manager
            .configure(
                "agent",
                &ConfigPatch {
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
        assert_eq!(
            result,
            Err(if apply_with_turn {
                AgentManagerError::Session
            } else {
                AgentManagerError::SessionRejected
            })
        );
        manager.close_all().await.unwrap();

        let record = registry.get("agent").unwrap().unwrap();
        assert_eq!(record.config.unwrap().thinking_option_id, None);
        let info = record.runtime_info.unwrap();
        assert_eq!(info.thinking_option_id.as_deref(), Some("medium"));
        assert!(!info.extra.unwrap().contains_key("configPending"));
        assert!(
            !record
                .persistence
                .unwrap()
                .metadata
                .unwrap()
                .contains_key("deferredConfig")
        );
        std::fs::remove_file(directory.path().join("rejected")).unwrap();
        manager = AgentManager::new(Box::new(registry));
        manager.register_client(Box::new(client)).unwrap();
        manager.resume("agent").await.unwrap();
        manager.close_all().await.unwrap();
        let rejected = std::fs::read_to_string(directory.path().join("requests.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .filter(|command| command["type"] == "set_thinking_level" && command["level"] == "high")
            .count();
        assert_eq!(rejected, 1);
    }
}

#[tokio::test]
async fn dormant_model_rejection_preserves_both_suppressed_preferences() {
    let (directory, client, mut spec) = fixture("reject-default-model");
    spec.config.model = Some("fixture/other".into());
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

    assert_eq!(
        manager
            .configure(
                "agent",
                &ConfigPatch {
                    model_id: NullableSetting::Set("fixture/model".into()),
                    ..ConfigPatch::default()
                },
            )
            .await,
        Err(AgentManagerError::SessionRejected)
    );

    let record = registry.get("agent").unwrap().unwrap();
    assert_eq!(record.config.as_ref(), Some(&spec.config));
    let info = record.runtime_info.unwrap();
    assert_eq!(info.model.as_deref(), Some("fixture/model"));
    assert_eq!(info.thinking_option_id.as_deref(), Some("medium"));
    assert!(!manager.has_sessions());
    manager = AgentManager::new(Box::new(registry));
    manager.register_client(Box::new(client)).unwrap();
    let resumed = manager.resume("agent").await.unwrap();
    assert_eq!(resumed.config.as_ref(), Some(&spec.config));
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn registration_readback_failure_preserves_consumed_deferred_clear() {
    let (directory, client, spec) = fixture("multiple-models");
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
    std::fs::write(directory.path().join("fail-registration"), "").unwrap();

    assert_eq!(
        manager.resume("agent").await,
        Err(AgentManagerError::Session)
    );

    let record = registry.get("agent").unwrap().unwrap();
    assert_eq!(record.config.unwrap().thinking_option_id, None);
    assert_eq!(
        record.runtime_info.unwrap().thinking_option_id.as_deref(),
        Some("medium")
    );
    assert!(
        !record
            .persistence
            .unwrap()
            .metadata
            .unwrap()
            .contains_key("deferredConfig")
    );
    let native_path = directory.path().join("session.state.json");
    let mut native: Value = serde_json::from_slice(&std::fs::read(&native_path).unwrap()).unwrap();
    native["thinking"] = json!("high");
    std::fs::write(native_path, serde_json::to_vec(&native).unwrap()).unwrap();
    manager = AgentManager::new(Box::new(registry));
    manager.register_client(Box::new(client)).unwrap();
    let resumed = manager.resume("agent").await.unwrap();
    assert_eq!(
        resumed.runtime_info.unwrap().thinking_option_id.as_deref(),
        Some("high")
    );
    manager.close_all().await.unwrap();
    let clears = std::fs::read_to_string(directory.path().join("requests.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|command| command["type"] == "set_thinking_level")
        .count();
    assert_eq!(clears, 1);
}
