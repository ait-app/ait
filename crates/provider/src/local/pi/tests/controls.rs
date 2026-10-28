use super::*;
use crate::ports::agent_session::ConfigApplication;
use crate::protocol::agent_config::{ConfigPatch, NullableSetting};
use crate::service::agent_manager::{AgentManager, AgentRegistration, ConfigurationOutcome};
use domain::agent_runtime::registry::AgentRuntimeRegistry;
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

#[tokio::test]
async fn active_turn_defers_controls_without_rpc_and_remains_usable() {
    let (directory, mut client, mut spec) = fixture("deferred-controls");
    client.deadline = Duration::from_secs(1);
    let mut session = client.create_session(&spec).await.unwrap();
    let original = session.effective_config().unwrap();
    let identity = session.persistence().unwrap().session_id;
    session
        .start_turn("delayed-turn", &spec.config)
        .await
        .unwrap();
    session.runtime_info().await.unwrap();
    let requests = directory.path().join("requests.jsonl");
    let before = std::fs::read_to_string(&requests).unwrap();
    for patch in [
        ConfigPatch {
            thinking_option_id: NullableSetting::Set("high".into()),
            ..ConfigPatch::default()
        },
        ConfigPatch {
            thinking_option_id: NullableSetting::Set("medium".into()),
            ..ConfigPatch::default()
        },
        ConfigPatch {
            thinking_option_id: NullableSetting::Clear,
            ..ConfigPatch::default()
        },
        ConfigPatch {
            model_id: NullableSetting::Set("fixture/other".into()),
            ..ConfigPatch::default()
        },
    ] {
        spec.config = patch.apply(&original);
        assert!(matches!(
            session.apply_config(&spec.config, &patch).await.unwrap(),
            ConfigApplication::Queued
        ));
        assert_eq!(session.effective_config().unwrap(), spec.config);
    }
    assert_eq!(std::fs::read_to_string(&requests).unwrap(), before);
    assert!(matches!(
        terminal(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(Some(text))) if text == "Extension"
    ));
    let patch = ConfigPatch {
        thinking_option_id: NullableSetting::Set("high".into()),
        ..ConfigPatch::default()
    };
    spec.config = patch.apply(&original);
    session.apply_config(&spec.config, &patch).await.unwrap();
    spec.config = session.effective_config().unwrap();
    assert_eq!(spec.config.thinking_option_id.as_deref(), Some("high"));
    assert_eq!(session.persistence().unwrap().session_id, identity);
    session.start_turn("hello", &spec.config).await.unwrap();
    assert!(matches!(
        terminal(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn manager_retains_next_turn_config_without_interrupting_the_active_turn() {
    let (directory, mut client, spec) = fixture("deferred-controls");
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
    manager.send("agent", "delayed-turn").await.unwrap();
    let original = registry.get("agent").unwrap().unwrap();
    let turn = manager.active_turn("agent").unwrap().to_owned();
    let requests = directory.path().join("requests.jsonl");
    let before = std::fs::read_to_string(&requests).unwrap();
    let patch = ConfigPatch {
        model_id: NullableSetting::Set("fixture/other".into()),
        thinking_option_id: NullableSetting::Set("high".into()),
        ..ConfigPatch::default()
    };
    assert_eq!(
        manager.configure("agent", &patch).await.unwrap(),
        ConfigurationOutcome::Deferred
    );
    let after = std::fs::read_to_string(&requests).unwrap();
    assert!(after[before.len()..].lines().all(|line| {
        let command: Value = serde_json::from_str(line).unwrap();
        !matches!(
            command["type"].as_str(),
            Some("set_model" | "set_thinking_level")
        )
    }));
    assert_eq!(
        registry.get("agent").unwrap().unwrap().config.unwrap(),
        patch.apply(original.config.as_ref().unwrap())
    );
    assert_eq!(manager.active_turn("agent"), Some(turn.as_str()));
    settle_manager_turn(&mut manager).await;
    assert!(registry.get("agent").unwrap().unwrap().last_error.is_none());
    manager.send("agent", "hello").await.unwrap();
    manager.poll().await.unwrap();
    let updated = registry.get("agent").unwrap().unwrap();
    assert_eq!(
        updated.persistence.unwrap().session_id,
        original.persistence.unwrap().session_id
    );
    assert_eq!(
        updated.config.unwrap().model.as_deref(),
        Some("fixture/other")
    );
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn controls_apply_before_any_prompt_and_persist_native_readback() {
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
    for (patch, model, thinking) in [
        (
            ConfigPatch {
                thinking_option_id: NullableSetting::Set("high".into()),
                ..ConfigPatch::default()
            },
            "fixture/model",
            "high",
        ),
        (
            ConfigPatch {
                model_id: NullableSetting::Set("fixture/other".into()),
                ..ConfigPatch::default()
            },
            "fixture/other",
            "low",
        ),
        (
            ConfigPatch {
                thinking_option_id: NullableSetting::Clear,
                ..ConfigPatch::default()
            },
            "fixture/other",
            "medium",
        ),
        (
            ConfigPatch {
                model_id: NullableSetting::Set("fixture/model".into()),
                thinking_option_id: NullableSetting::Set("minimal".into()),
                ..ConfigPatch::default()
            },
            "fixture/model",
            "minimal",
        ),
    ] {
        assert_eq!(
            manager.configure("agent", &patch).await.unwrap(),
            ConfigurationOutcome::Applied
        );
        let record = registry.get("agent").unwrap().unwrap();
        let config = record.config.unwrap();
        let info = record.runtime_info.unwrap();
        assert_eq!(config.model.as_deref(), Some(model));
        assert_eq!(info.model.as_deref(), Some(model));
        assert_eq!(
            config.thinking_option_id.as_deref(),
            (patch.thinking_option_id != NullableSetting::Clear).then_some(thinking)
        );
        assert_eq!(info.thinking_option_id.as_deref(), Some(thinking));
        assert_eq!(native_thinking(&info), Some(thinking));
    }
    let commands = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    assert!(
        commands
            .lines()
            .all(|line| serde_json::from_str::<Value>(line).unwrap()["type"] != "prompt")
    );
    manager.send("agent", "hello").await.unwrap();
    assert!(manager.active_turn("agent").is_some());
    settle_manager_turn(&mut manager).await;
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
            .unwrap(),
        ConfigurationOutcome::Applied
    );
    // Later polling must not restore a runtime level observed before this control.
    manager.poll().await.unwrap();
    let record = registry.get("agent").unwrap().unwrap();
    assert_eq!(
        record.runtime_info.unwrap().thinking_option_id.as_deref(),
        Some("high")
    );
    assert_eq!(
        record.config.unwrap().thinking_option_id.as_deref(),
        Some("high")
    );
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn clamping_extensions_reselection_and_reload_keep_saved_controls_effective() {
    let (directory, client, mut spec) = fixture("clamp-thinking");
    spec.config.model = Some("fixture/model".into());
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
    assert_eq!(
        manager
            .configure(
                "agent",
                &ConfigPatch {
                    thinking_option_id: NullableSetting::Set("max".into()),
                    ..ConfigPatch::default()
                }
            )
            .await
            .unwrap(),
        ConfigurationOutcome::Applied
    );
    assert_eq!(
        registry
            .get("agent")
            .unwrap()
            .unwrap()
            .config
            .unwrap()
            .thinking_option_id
            .as_deref(),
        Some("high")
    );
    // Materialize native history before the extension-only command.
    manager.send("agent", "hello").await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            manager.poll().await.unwrap();
            if manager.active_turn("agent").is_none() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    manager.send("agent", "thinking-after").await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            manager.poll().await.unwrap();
            if manager.active_turn("agent").is_none() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    let record = registry.get("agent").unwrap().unwrap();
    assert_eq!(
        record.config.unwrap().thinking_option_id.as_deref(),
        Some("low")
    );
    assert_eq!(
        record.runtime_info.unwrap().thinking_option_id.as_deref(),
        Some("low")
    );
    manager.close_all().await.unwrap();
    let mut resumed = AgentManager::new(Box::new(registry.clone()));
    resumed.register_client(Box::new(client)).unwrap();
    let record = resumed
        .restore("agent", &crate::protocol::resume::Overrides::default())
        .await
        .unwrap();
    // The restored native runtime may clamp Low again; persist its actual High readback.
    assert_eq!(
        record.config.unwrap().thinking_option_id.as_deref(),
        Some("high")
    );
    assert_eq!(
        record.runtime_info.unwrap().thinking_option_id.as_deref(),
        Some("high")
    );
    assert_eq!(
        resumed
            .configure(
                "agent",
                &ConfigPatch {
                    thinking_option_id: NullableSetting::Set("high".into()),
                    ..ConfigPatch::default()
                }
            )
            .await
            .unwrap(),
        ConfigurationOutcome::Applied
    );
    resumed.close_all().await.unwrap();
}

#[tokio::test]
async fn failed_thinking_control_still_publishes_a_successful_model_switch() {
    let (directory, client, spec) = fixture("reject-thinking-after-model");
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
    assert!(
        manager
            .configure(
                "agent",
                &ConfigPatch {
                    model_id: NullableSetting::Set("fixture/other".into()),
                    thinking_option_id: NullableSetting::Set("high".into()),
                    ..ConfigPatch::default()
                }
            )
            .await
            .is_err()
    );
    manager.poll().await.unwrap();
    let record = registry.get("agent").unwrap().unwrap();
    let info = record.runtime_info.unwrap();
    let config = record.config.unwrap();
    assert_eq!(info.model.as_deref(), Some("fixture/other"));
    assert_eq!(config.model, info.model);
    assert_eq!(config.thinking_option_id, None);
    assert_eq!(info.thinking_option_id.as_deref(), Some("low"));
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn resuming_an_already_selected_model_does_not_reset_native_thinking() {
    let (directory, client, mut spec) = fixture("multiple-models");
    spec.config.model = Some("fixture/model".into());
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("thinking-after", &spec.config)
        .await
        .unwrap();
    terminal(session.as_mut()).await;
    session.start_turn("hello", &spec.config).await.unwrap();
    terminal(session.as_mut()).await;
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let before = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert_eq!(
        native_thinking(&resumed.runtime_info().await.unwrap()),
        Some("low")
    );
    let after = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    assert!(after[before.len()..].lines().all(|line| {
        let command: Value = serde_json::from_str(line).unwrap();
        !matches!(
            command["type"].as_str(),
            Some("set_model" | "set_thinking_level")
        )
    }));
    resumed.close().await.unwrap();
}

#[tokio::test]
async fn model_switches_apply_through_the_worker_after_restart_without_a_prompt() {
    use crate::test_support::{Fixture, request, worker_with_client};

    let host = Fixture::new();
    let (_native, client, _) = fixture("multiple-models");
    let (execution, _) = worker_with_client(
        &host,
        model::creation::Creations::default(),
        Box::new(client.clone()),
    );
    let created = request(
        &execution,
        "agent.create.request",
        json!({
            "idempotencyKey":"pi-model-switch",
            "env":{"PI_FIXTURE_SCENARIO":"multiple-models"},
            "config":{"provider":"pi","cwd":host.cwd,"model":"fixture/model"}
        }),
    )
    .await;
    let id = created["agentId"].as_str().unwrap();
    let changed = request(
        &execution,
        "agent.model.set.request",
        json!({
            "agentId":id,"modelId":"fixture/other"
        }),
    )
    .await;
    assert_eq!(changed["accepted"], true, "{changed}");
    let snapshot = request(&execution, "agent.get.request", json!({"agentId":id})).await;
    assert_eq!(snapshot["agent"]["runtimeInfo"]["model"], "fixture/other");
    execution.shutdown().await.unwrap();

    let (restarted, _) = worker_with_client(
        &host,
        model::creation::Creations::default(),
        Box::new(client.clone()),
    );
    for model in ["fixture/model", "fixture/other", "fixture/model"] {
        let changed = request(
            &restarted,
            "agent.model.set.request",
            json!({
                "agentId":id,"modelId":model
            }),
        )
        .await;
        assert_eq!(changed["accepted"], true, "{changed}");
        let snapshot = request(&restarted, "agent.get.request", json!({"agentId":id})).await;
        assert_eq!(snapshot["agent"]["model"], model);
        assert_eq!(
            snapshot["agent"]["runtimeInfo"]["model"], model,
            "an accepted model switch must update Pi before a prompt, including after restart"
        );
    }
    restarted.shutdown().await.unwrap();
    let (restarted, _) = worker_with_client(
        &host,
        model::creation::Creations::default(),
        Box::new(client),
    );
    let changed = request(
        &restarted,
        "agent.thinking.set.request",
        json!({
            "agentId":id,"thinkingOptionId":"high"
        }),
    )
    .await;
    assert_eq!(changed["accepted"], true, "{changed}");
    let snapshot = request(&restarted, "agent.get.request", json!({"agentId":id})).await;
    assert_eq!(snapshot["agent"]["thinkingOptionId"], "high");
    assert_eq!(snapshot["agent"]["runtimeInfo"]["thinkingOptionId"], "high");
    let commands = std::fs::read_to_string(host.cwd.join("requests.jsonl")).unwrap();
    assert!(
        commands
            .lines()
            .all(|line| serde_json::from_str::<Value>(line).unwrap()["type"] != "prompt")
    );
    restarted.shutdown().await.unwrap();
}

#[tokio::test]
async fn dormant_native_controls_reject_failed_resume_without_saving_a_deferred_request() {
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
    manager.send("agent", "hello").await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            manager.poll().await.unwrap();
            if manager.active_turn("agent").is_none() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    manager.close_all().await.unwrap();
    let original = registry.get("agent").unwrap().unwrap();
    std::fs::remove_file(directory.path().join("session.jsonl")).unwrap();
    let result = manager
        .configure(
            "agent",
            &ConfigPatch {
                model_id: NullableSetting::Set("fixture/other".into()),
                ..ConfigPatch::default()
            },
        )
        .await;
    assert!(result.is_err());
    assert_eq!(registry.get("agent").unwrap().unwrap(), original);
    assert!(manager.live_snapshot("agent").is_none());
}

#[tokio::test]
async fn control_waits_project_large_stream_bursts_without_losing_output_or_settlement() {
    for target in ["abort", "abort-rejected"] {
        let (_directory, client, mut spec) = fixture(&format!("control-burst-{target}"));
        let mut session = client.create_session(&spec).await.unwrap();
        session.runtime_info().await.unwrap();
        let turn = session.start_turn("wait", &spec.config).await.unwrap();
        let result = session.cancel_turn(&turn).await;
        assert_eq!(result.is_err(), target == "abort-rejected");
        let events = terminal(session.as_mut()).await;
        let text = events
            .iter()
            .find_map(|event| match event {
                AgentTurnEvent::Timeline(entry) if entry.item["type"] == "assistant_message" => {
                    entry.item["text"].as_str()
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(text, "x".repeat(5000), "{target}");
        let tools: std::collections::BTreeSet<_> = events
            .iter()
            .filter_map(|event| match event {
                AgentTurnEvent::Timeline(entry) if entry.item["type"] == "tool_call" => {
                    assert_eq!(entry.item["status"], "completed");
                    Some(entry.key.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(tools.len(), 300, "{target}");
        assert!(matches!(events.last(), Some(AgentTurnEvent::Cancelled)));
        spec.config = session.effective_config().unwrap();
        session.start_turn("hello", &spec.config).await.unwrap();
        assert!(matches!(
            terminal(session.as_mut()).await.last(),
            Some(AgentTurnEvent::Completed(_))
        ));
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn control_projection_still_rejects_output_beyond_its_bounded_capacity() {
    let (_directory, client, spec) = fixture("control-burst-limit");
    let mut session = client.create_session(&spec).await.unwrap();
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    assert!(session.cancel_turn(&turn).await.is_err());
    // Drain the bounded backlog directly: the shared helper deliberately sleeps between
    // polls, which would turn a full 4096-event queue into an unrelated timeout.
    let failed =
        (0..8192).any(|_| matches!(session.poll_turn().unwrap(), Some(AgentTurnEvent::Failed)));
    assert!(failed);
    assert_eq!(
        session.failure_diagnostic().unwrap().source,
        "display_limit"
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn thinking_only_patches_work_when_catalog_discovery_is_unavailable() {
    let (directory, client, spec) = fixture("discovery-models-rejected");
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
    for level in [NullableSetting::Set("ultra".into()), NullableSetting::Clear] {
        // Closing exercises the same dormant path used after idle expiry or restart.
        manager.close_all().await.unwrap();
        manager
            .configure(
                "agent",
                &ConfigPatch {
                    thinking_option_id: level,
                    ..ConfigPatch::default()
                },
            )
            .await
            .unwrap();
    }
    assert_eq!(
        registry
            .get("agent")
            .unwrap()
            .unwrap()
            .config
            .unwrap()
            .thinking_option_id
            .as_deref(),
        None
    );
    assert!(
        !std::fs::read_to_string(directory.path().join("requests.jsonl"))
            .unwrap()
            .contains("get_available_models")
    );
    // Explicit model selections still require catalog validation, including reselections.
    assert!(
        manager
            .configure(
                "agent",
                &ConfigPatch {
                    model_id: NullableSetting::Set("fixture/model".into()),
                    ..ConfigPatch::default()
                }
            )
            .await
            .is_err()
    );
    assert!(
        manager
            .configure(
                "agent",
                &ConfigPatch {
                    thinking_option_id: NullableSetting::Set("bad\nlevel".into()),
                    ..ConfigPatch::default()
                }
            )
            .await
            .is_err()
    );
    manager.close_all().await.unwrap();
}

mod deferred;
mod lifecycle;
mod ordering;
mod preferences;
mod publication;
mod recovery;

#[tokio::test]
async fn rejected_control_takes_precedence_over_failed_readback() {
    let (_directory, client, mut spec) = fixture("reject-thinking-readback-exit");
    let mut session = client.create_session(&spec).await.unwrap();
    spec.config.thinking_option_id = Some("high".into());
    let result = session
        .apply_config(
            &spec.config,
            &ConfigPatch {
                thinking_option_id: NullableSetting::Set("high".into()),
                ..ConfigPatch::default()
            },
        )
        .await;
    assert!(matches!(result, Err(AgentSessionError::Rejected)));
    let _ = session.close().await;
}

#[tokio::test]
async fn dormant_model_patch_discovers_catalog_only_once() {
    let (directory, client, spec) = fixture("multiple-models");
    let registry = FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json"));
    let mut manager = AgentManager::new(Box::new(registry));
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
    let requests = directory.path().join("requests.jsonl");
    std::fs::write(&requests, "").unwrap();
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
    let commands = std::fs::read_to_string(requests).unwrap();
    assert_eq!(
        commands
            .lines()
            .filter(|line| {
                serde_json::from_str::<Value>(line).unwrap()["type"] == "get_available_models"
            })
            .count(),
        1
    );
    manager.close_all().await.unwrap();
}

async fn settle_manager_turn(manager: &mut AgentManager) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while manager.active_turn("agent").is_some() {
            manager.poll().await.unwrap();
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
}
