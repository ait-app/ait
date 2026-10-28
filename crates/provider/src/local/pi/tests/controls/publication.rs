//! Native control publication transfers ownership to the manager's retryable snapshot.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use super::*;
use domain::agent_runtime::{
    PersistedAgentRuntimeRecord,
    registry::{AgentRuntimeRegistry, AgentRuntimeRegistryError},
};

#[derive(Debug, Clone)]
struct Registry {
    inner: FileBackedAgentRuntimeRegistry,
    fail: Arc<AtomicBool>,
    updates: Arc<AtomicUsize>,
    fail_from: Arc<AtomicUsize>,
}

#[tokio::test]
async fn one_streamed_runtime_observation_is_written_once_and_does_not_block_completion() {
    let (directory, client, spec) = fixture("");
    let registry = Registry {
        inner: FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json")),
        fail: Arc::default(),
        updates: Arc::default(),
        fail_from: Arc::default(),
    };
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
    let before = registry.updates.load(Ordering::SeqCst);
    manager.cancel("agent").await.unwrap();
    manager.poll().await.unwrap();
    assert!(manager.active_turn("agent").is_none());
    // Cancellation with no runtime change commits only its terminal status.
    assert_eq!(registry.updates.load(Ordering::SeqCst) - before, 1);
    manager.send("agent", "thinking-after").await.unwrap();
    // The abort acknowledgement is an ordering barrier: the notification and settlement
    // preceding it are now projected and ready for the same manager poll.
    manager.cancel("agent").await.unwrap();
    let before = registry.updates.load(Ordering::SeqCst);
    manager.poll().await.unwrap();
    assert!(manager.active_turn("agent").is_none());
    // Pending admission facts, one streamed runtime observation, and terminal status.
    assert_eq!(registry.updates.load(Ordering::SeqCst) - before, 3);
    manager.close_all().await.unwrap();
}

impl AgentRuntimeRegistry for Registry {
    fn initialize(&self) -> Result<(), AgentRuntimeRegistryError> {
        self.inner.initialize()
    }
    fn list(&self) -> Result<Vec<PersistedAgentRuntimeRecord>, AgentRuntimeRegistryError> {
        self.inner.list()
    }
    fn get(
        &self,
        id: &str,
    ) -> Result<Option<PersistedAgentRuntimeRecord>, AgentRuntimeRegistryError> {
        self.inner.get(id)
    }
    fn upsert(
        &self,
        record: &PersistedAgentRuntimeRecord,
    ) -> Result<(), AgentRuntimeRegistryError> {
        self.inner.upsert(record)
    }
    fn update(
        &self,
        id: &str,
        update: &dyn Fn(&PersistedAgentRuntimeRecord) -> PersistedAgentRuntimeRecord,
    ) -> Result<Option<PersistedAgentRuntimeRecord>, AgentRuntimeRegistryError> {
        let fail_from = self.fail_from.load(Ordering::SeqCst);
        if self.fail.swap(false, Ordering::SeqCst)
            || fail_from != 0 && self.updates.load(Ordering::SeqCst) >= fail_from
        {
            return Err(AgentRuntimeRegistryError::Io);
        }
        let result = self.inner.update(id, update)?;
        self.updates.fetch_add(1, Ordering::SeqCst);
        Ok(result)
    }
    fn remove(&self, id: &str) -> Result<bool, AgentRuntimeRegistryError> {
        self.inner.remove(id)
    }
}

#[tokio::test]
async fn rejected_deferred_registration_retries_storage_without_replaying_native_controls() {
    use crate::service::agent_manager::AgentManagerError;

    for cleanup in ["poll", "close", "resume"] {
        let (directory, client, spec) = fixture("reject-thinking-after-model");
        let registry = Registry {
            inner: FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json")),
            fail: Arc::default(),
            updates: Arc::default(),
            fail_from: Arc::default(),
        };
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
        manager.close_all().await.unwrap();
        registry.fail.store(true, Ordering::SeqCst);

        assert_eq!(
            manager.resume("agent").await,
            Err(AgentManagerError::Registry)
        );
        assert!(manager.has_sessions());
        assert!(!lifecycle::process_running(&directory));
        if cleanup == "poll" {
            // Keep storage unavailable for another cleanup attempt.
            registry.fail.store(true, Ordering::SeqCst);
            manager.poll().await.unwrap();
            assert!(manager.has_sessions());
            manager.poll().await.unwrap();
        } else if cleanup == "close" {
            manager.close_all().await.unwrap();
        } else {
            manager.resume("agent").await.unwrap();
            manager.close_all().await.unwrap();
        }
        assert!(!manager.has_sessions());
        let record = registry.get("agent").unwrap().unwrap();
        assert_eq!(
            record.config.unwrap().model.as_deref(),
            Some("fixture/other")
        );
        assert_eq!(
            record.runtime_info.unwrap().thinking_option_id.as_deref(),
            Some("low")
        );
        assert!(
            !record
                .persistence
                .unwrap()
                .metadata
                .unwrap()
                .contains_key("deferredConfig")
        );

        // Verify the consumed command stays consumed after restarting the manager.
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
        assert_eq!(rejected, 1, "{cleanup}");
    }
}

#[tokio::test]
async fn successful_deferred_registration_retries_without_replaying_consumed_controls() {
    use crate::service::agent_manager::AgentManagerError;

    for cleanup in ["poll", "close", "resume"] {
        let (directory, client, spec) = fixture("multiple-models");
        let registry = Registry {
            inner: FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json")),
            fail: Arc::default(),
            updates: Arc::default(),
            fail_from: Arc::default(),
        };
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
                    thinking_option_id: NullableSetting::Clear,
                    ..ConfigPatch::default()
                },
            )
            .await
            .unwrap();
        manager.close_all().await.unwrap();
        registry.fail.store(true, Ordering::SeqCst);

        assert_eq!(
            manager.resume("agent").await,
            Err(AgentManagerError::Registry)
        );
        assert!(manager.has_sessions(), "{cleanup}");
        assert!(!lifecycle::process_running(&directory));
        // Independent native changes after the failed write must survive retrying storage.
        let native_state_path = directory.path().join("session.state.json");
        let mut native_state: Value =
            serde_json::from_str(&std::fs::read_to_string(&native_state_path).unwrap()).unwrap();
        native_state["thinking"] = json!("ultra");
        std::fs::write(
            &native_state_path,
            serde_json::to_vec(&native_state).unwrap(),
        )
        .unwrap();
        if cleanup == "poll" {
            registry.fail.store(true, Ordering::SeqCst);
            manager.poll().await.unwrap();
            assert!(manager.has_sessions());
            manager.poll().await.unwrap();
        } else if cleanup == "close" {
            manager.close_all().await.unwrap();
        } else {
            manager.resume("agent").await.unwrap();
            manager.close_all().await.unwrap();
        }
        assert!(!manager.has_sessions());
        let record = registry.get("agent").unwrap().unwrap();
        let config = record.config.unwrap();
        assert_eq!(config.model.as_deref(), Some("fixture/other"));
        assert_eq!(config.thinking_option_id, None);
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
        let resumed = manager.resume("agent").await.unwrap();
        assert_eq!(
            resumed.runtime_info.unwrap().thinking_option_id.as_deref(),
            Some("ultra")
        );
        manager.close_all().await.unwrap();
        let commands: Vec<Value> = std::fs::read_to_string(directory.path().join("requests.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        for kind in ["set_model", "set_thinking_level"] {
            assert_eq!(
                commands
                    .iter()
                    .filter(|command| command["type"] == kind)
                    .count(),
                1,
                "{cleanup}: {kind}"
            );
        }
    }
}

#[tokio::test]
async fn deferred_clear_consumption_commits_atomically_before_restart() {
    let (directory, client, spec) = fixture("multiple-models");
    let registry = Registry {
        inner: FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json")),
        fail: Arc::default(),
        updates: Arc::default(),
        fail_from: Arc::default(),
    };
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
    manager.cancel("agent").await.unwrap();
    settle_manager_turn(&mut manager).await;
    manager.send("agent", "wait").await.unwrap();
    // Permit the runtime update; every subsequent write fails until restart.
    registry.fail_from.store(
        registry.updates.load(Ordering::SeqCst) + 1,
        Ordering::SeqCst,
    );

    let result = manager.poll().await;
    assert!(matches!(
        result,
        Ok(()) | Err(crate::service::agent_manager::AgentManagerError::Registry)
    ));

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
    // Drop ownership without graceful persistence, as on daemon shutdown after an error.
    drop(manager);
    tokio::time::timeout(Duration::from_secs(5), async {
        while lifecycle::process_running(&directory) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let state_path = directory.path().join("session.state.json");
    let mut state: Value =
        serde_json::from_str(&std::fs::read_to_string(&state_path).unwrap()).unwrap();
    state["thinking"] = json!("ultra");
    std::fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();
    registry.fail_from.store(0, Ordering::SeqCst);
    manager = AgentManager::new(Box::new(registry));
    manager.register_client(Box::new(client)).unwrap();
    let resumed = manager.resume("agent").await.unwrap();
    assert_eq!(
        resumed.runtime_info.unwrap().thinking_option_id.as_deref(),
        Some("ultra")
    );
    manager.close_all().await.unwrap();
    let resets = std::fs::read_to_string(directory.path().join("requests.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|command| command["type"] == "set_thinking_level")
        .count();
    assert_eq!(resets, 1);
}

#[tokio::test]
async fn successful_controls_publish_once_and_failed_storage_retries_without_native_replay() {
    for fail in [false, true] {
        let (directory, client, spec) = fixture("");
        let registry = Registry {
            inner: FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json")),
            fail: Arc::default(),
            updates: Arc::default(),
            fail_from: Arc::default(),
        };
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
        let before = registry.updates.load(Ordering::SeqCst);
        registry.fail.store(fail, Ordering::SeqCst);
        assert_eq!(
            manager
                .configure(
                    "agent",
                    &ConfigPatch {
                        thinking_option_id: NullableSetting::Set("high".into()),
                        ..ConfigPatch::default()
                    }
                )
                .await
                .is_err(),
            fail
        );
        manager.poll().await.unwrap();
        manager.poll().await.unwrap();
        assert_eq!(registry.updates.load(Ordering::SeqCst) - before, 1);
        let record = registry.get("agent").unwrap().unwrap();
        assert_eq!(
            record.config.unwrap().thinking_option_id.as_deref(),
            Some("high")
        );
        assert_eq!(
            record.runtime_info.unwrap().thinking_option_id.as_deref(),
            Some("high")
        );
        let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
        assert_eq!(
            requests
                .lines()
                .filter(|line| line.contains("set_thinking_level"))
                .count(),
            1
        );
        manager.close_all().await.unwrap();
    }
}

#[tokio::test]
async fn dormant_control_storage_failure_closes_the_process_and_retries_without_native_replay() {
    let (directory, client, spec) = fixture("");
    let registry = Registry {
        inner: FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json")),
        fail: Arc::default(),
        updates: Arc::default(),
        fail_from: Arc::default(),
    };
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
    // Allow resume registration, then keep storage unavailable through native mutation
    // and cleanup so the closed session must retain its pending snapshot for polling.
    registry.fail_from.store(
        registry.updates.load(Ordering::SeqCst) + 1,
        Ordering::SeqCst,
    );
    let result = manager
        .configure(
            "agent",
            &ConfigPatch {
                thinking_option_id: NullableSetting::Set("high".into()),
                ..ConfigPatch::default()
            },
        )
        .await;
    assert_eq!(
        result,
        Err(crate::service::agent_manager::AgentManagerError::Registry)
    );
    assert!(!lifecycle::process_running(&directory));
    assert!(manager.has_sessions());
    manager.reconcile().await.unwrap();
    manager.poll().await.unwrap();
    assert!(manager.has_sessions());
    assert!(!lifecycle::process_running(&directory));
    registry.fail_from.store(0, Ordering::SeqCst);
    manager.poll().await.unwrap();
    assert!(!manager.has_sessions());
    let record = registry.get("agent").unwrap().unwrap();
    assert_eq!(
        record.config.unwrap().thinking_option_id.as_deref(),
        Some("high")
    );
    assert_eq!(
        record.runtime_info.unwrap().thinking_option_id.as_deref(),
        Some("high")
    );
    let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    assert_eq!(
        requests
            .lines()
            .filter(|line| line.contains("set_thinking_level"))
            .count(),
        1
    );
}

#[tokio::test]
async fn restore_flushes_failed_thinking_clear_before_deriving_settings() {
    use crate::protocol::resume::Overrides;
    use crate::service::agent_manager::AgentManagerError;

    for thinking_override in [None, Some("low")] {
        let (directory, client, mut spec) = fixture("");
        spec.config.thinking_option_id = Some("high".into());
        let registry = Registry {
            inner: FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json")),
            fail: Arc::default(),
            updates: Arc::default(),
            fail_from: Arc::default(),
        };
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
        manager.send("agent", "hello").await.unwrap();
        settle_manager_turn(&mut manager).await;
        manager.close_all().await.unwrap();
        registry.fail_from.store(
            registry.updates.load(Ordering::SeqCst) + 1,
            Ordering::SeqCst,
        );
        assert_eq!(
            manager
                .configure(
                    "agent",
                    &ConfigPatch {
                        thinking_option_id: NullableSetting::Clear,
                        ..ConfigPatch::default()
                    },
                )
                .await,
            Err(AgentManagerError::Registry)
        );
        let overrides = Overrides {
            config: StoredAgentConfig {
                thinking_option_id: thinking_override.map(str::to_owned),
                ..StoredAgentConfig::default()
            },
            ..Overrides::default()
        };
        assert_eq!(
            manager.restore("agent", &overrides).await,
            Err(AgentManagerError::Registry)
        );
        assert!(!lifecycle::process_running(&directory));
        registry.fail_from.store(0, Ordering::SeqCst);

        let restored = manager.restore("agent", &overrides).await.unwrap();

        assert_eq!(
            restored.config.unwrap().thinking_option_id.as_deref(),
            thinking_override
        );
        assert_eq!(
            restored.runtime_info.unwrap().thinking_option_id.as_deref(),
            Some(thinking_override.unwrap_or("medium"))
        );
        manager.close_all().await.unwrap();
    }
}
