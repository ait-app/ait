use super::*;

#[tokio::test]
async fn thinking_rejection_survives_conflicting_readback() {
    for opening in [false, true] {
        let (directory, mut client, mut spec) = fixture("reject-thinking");
        client.deadline = Duration::from_secs(2);
        std::fs::write(directory.path().join("rejection-readback"), "conflict").unwrap();
        if opening {
            spec.config.model = Some("fixture/other".into());
            spec.config.thinking_option_id = Some("high".into());
        }
        let mut session = match client.create_session(&spec).await {
            Ok(session) => session,
            Err(error) => panic!(
                "opening={opening}: {error:?}; diagnostic={:?}",
                client.diagnostic().await
            ),
        };
        spec.config.model = Some("fixture/other".into());
        spec.config.thinking_option_id = Some("high".into());

        assert_eq!(
            session.start_turn("keep this", &spec.config).await,
            Err(AgentSessionError::Rejected)
        );

        assert_eq!(
            session.failure_diagnostic().unwrap().source,
            "thinking_configuration"
        );
        assert_eq!(
            session.failure_message().unwrap(),
            "Pi: Pi rejected the thinking level. Choose a supported level before retrying. The prompt was not submitted."
        );
        let info = session.runtime_info().await.unwrap();
        assert_eq!(info.model.as_deref(), Some("fixture/other"));
        assert_eq!(native_thinking(&info), Some("low"));
        session.close().await.unwrap();
        let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
        let commands: Vec<Value> = requests
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap()["type"].clone())
            .collect();
        assert!(!commands.contains(&json!("prompt")));
        let readback_commands: Vec<_> = commands
            .iter()
            .skip_while(|command| **command != "set_thinking_level")
            .skip(1)
            .collect();
        assert_eq!(readback_commands, vec![&json!("get_state"); 3]);
    }
}

#[tokio::test]
async fn busy_admission_persists_native_changes_before_and_after_controls() {
    use crate::service::agent_manager::{AgentManager, AgentManagerError, AgentRegistration};
    use domain::agent_runtime::registry::AgentRuntimeRegistry;
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
    use provider_core::protocol::agent_config::{ConfigPatch, NullableSetting};

    for scenario in [
        "busy-runtime-change",
        "busy-runtime-compacting",
        "control-starts-work",
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
        if scenario == "control-starts-work" {
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
        }

        assert_eq!(
            manager.send("agent", "keep this").await,
            Err(AgentManagerError::Session)
        );

        let record = registry.get("agent").unwrap().unwrap();
        let info = record.runtime_info.unwrap();
        assert_eq!(info.model.as_deref(), Some("fixture/other"));
        assert_eq!(
            native_thinking(&info),
            Some(if scenario == "control-starts-work" {
                "high"
            } else {
                "low"
            })
        );
        assert!(record.last_error.is_some());
        let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
        assert!(
            requests
                .lines()
                .all(|line| serde_json::from_str::<Value>(line).unwrap()["type"] != "prompt")
        );
        manager.close_all().await.unwrap();
    }
}

#[tokio::test]
async fn native_error_wording_cannot_override_thinking_rejections() {
    for message in [
        "forbidden",
        "access denied",
        "model is unavailable",
        "timed out",
        "rate limit",
    ] {
        for opening in [false, true] {
            let (directory, client, mut spec) = fixture("worded-thinking-rejection");
            std::fs::write(directory.path().join("rejection-message"), message).unwrap();
            if opening {
                spec.config.thinking_option_id = Some("high".into());
            }
            let mut session = client.create_session(&spec).await.unwrap();
            spec.config.thinking_option_id = Some("high".into());

            assert_eq!(
                session.start_turn("keep this", &spec.config).await,
                Err(AgentSessionError::Rejected)
            );

            assert_eq!(
                session.failure_diagnostic().unwrap().source,
                "thinking_configuration"
            );
            assert_eq!(
                session.failure_message().unwrap(),
                "Pi: Pi rejected the thinking level. Choose a supported level before retrying. The prompt was not submitted."
            );
            session.close().await.unwrap();
            if opening {
                let handle = session.persistence().unwrap();
                let mut resumed = client
                    .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
                    .await
                    .unwrap();
                assert_eq!(
                    resumed.start_turn("keep this", &spec.config).await,
                    Err(AgentSessionError::Rejected)
                );
                assert_eq!(
                    resumed.failure_diagnostic().unwrap().source,
                    "thinking_configuration"
                );
                resumed.close().await.unwrap();
            }
        }
    }
}

#[tokio::test]
async fn opening_rejection_survives_unrelated_changes_and_repeated_prompts() {
    let (directory, client, mut spec) = fixture("worded-thinking-rejection");
    std::fs::write(directory.path().join("rejection-message"), "forbidden").unwrap();
    spec.config.thinking_option_id = Some("high".into());
    let mut session = client.create_session(&spec).await.unwrap();
    spec.config.mode_id = Some("default".into());
    spec.config.mcp_servers = Some(BTreeMap::new());
    for model in [None, Some("fixture/other"), Some("fixture/other")] {
        spec.config.model = model.map(str::to_owned);
        assert_eq!(
            session.start_turn("keep this", &spec.config).await,
            Err(AgentSessionError::Rejected)
        );
        assert_eq!(
            session.failure_diagnostic().unwrap().source,
            "thinking_configuration"
        );
    }
    let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    let controls: Vec<Value> = requests
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .filter(|command: &Value| command["type"] != "get_state")
        .collect();
    assert_eq!(controls.len(), 1);
    assert_eq!(controls[0]["type"], "set_thinking_level");

    spec.config.thinking_option_id = Some("low".into());
    session.start_turn("corrected", &spec.config).await.unwrap();
    assert!(matches!(
        terminal(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    assert_eq!(
        session.runtime_info().await.unwrap().model.as_deref(),
        Some("fixture/other")
    );
    assert!(session.failure_diagnostic().is_none());
    session.close().await.unwrap();
}

#[tokio::test]
async fn clearing_an_override_accepts_native_state_when_the_fallback_is_refused() {
    for model_changed in [false, true] {
        let (directory, client, mut spec) = fixture("reject-implicit-thinking");
        spec.config.thinking_option_id = Some("high".into());
        let mut session = client.create_session(&spec).await.unwrap();
        spec.config.thinking_option_id = None;
        if model_changed {
            spec.config.model = Some("fixture/other".into());
        }
        for _ in 0..2 {
            session
                .start_turn("no override", &spec.config)
                .await
                .unwrap();
            assert!(matches!(
                terminal(session.as_mut()).await.last(),
                Some(AgentTurnEvent::Completed(_))
            ));
            assert_eq!(
                native_thinking(&session.runtime_info().await.unwrap()),
                Some(if model_changed { "low" } else { "high" })
            );
            assert!(session.failure_diagnostic().is_none());
            assert!(session.failure_message().is_none());
        }
        session.close().await.unwrap();

        let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
        let levels: Vec<Value> = requests
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .filter(|command| command["type"] == "set_thinking_level")
            .map(|command| command["level"].clone())
            .collect();
        assert_eq!(levels, [json!("high"), json!("medium")]);
    }
}

#[tokio::test]
async fn an_explicitly_selected_fallback_level_still_reports_rejection() {
    let (_directory, client, mut spec) = fixture("reject-implicit-thinking");
    let mut session = client.create_session(&spec).await.unwrap();
    spec.config.thinking_option_id = Some("medium".into());

    assert_eq!(
        session.start_turn("keep this", &spec.config).await,
        Err(AgentSessionError::Rejected)
    );

    assert_eq!(
        session.failure_diagnostic().unwrap().source,
        "thinking_configuration"
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn clearing_an_opening_rejection_releases_the_preference_without_resending_it() {
    let (directory, client, mut spec) = fixture("worded-thinking-rejection");
    std::fs::write(directory.path().join("rejection-message"), "forbidden").unwrap();
    spec.config.thinking_option_id = Some("high".into());
    let mut session = client.create_session(&spec).await.unwrap();
    spec.config.thinking_option_id = None;

    session
        .start_turn("no override", &spec.config)
        .await
        .unwrap();

    assert!(matches!(
        terminal(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    assert!(session.failure_diagnostic().is_none());
    session.close().await.unwrap();
    let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    let levels: Vec<Value> = requests
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|command| command["type"] == "set_thinking_level")
        .map(|command| command["level"].clone())
        .collect();
    assert_eq!(levels, [json!("high"), json!("medium")]);
}

#[tokio::test]
async fn clearing_an_override_still_fails_on_uncertain_native_control_errors() {
    let (directory, client, mut spec) = fixture("reject-implicit-thinking");
    spec.config.thinking_option_id = Some("high".into());
    let mut session = client.create_session(&spec).await.unwrap();
    spec.config.thinking_option_id = None;
    std::fs::write(directory.path().join("disconnect-thinking"), "").unwrap();

    assert_eq!(
        session.start_turn("keep this", &spec.config).await,
        Err(AgentSessionError::Failed)
    );

    let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    assert!(
        requests
            .lines()
            .all(|line| serde_json::from_str::<Value>(line).unwrap()["type"] != "prompt")
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn transport_failures_during_rejection_readback_retire_the_writer() {
    for readback in ["timeout", "exit"] {
        for opening in [false, true] {
            let (directory, mut client, mut spec) = fixture("reject-thinking");
            client.deadline = Duration::from_millis(500);
            std::fs::write(directory.path().join("rejection-readback"), readback).unwrap();
            if opening {
                spec.config.thinking_option_id = Some("high".into());
                assert!(matches!(
                    client.create_session(&spec).await,
                    Err(AgentSessionError::Failed)
                ));
            } else {
                let mut session = client.create_session(&spec).await.unwrap();
                spec.config.thinking_option_id = Some("high".into());
                assert_eq!(
                    session.start_turn("keep this", &spec.config).await,
                    Err(AgentSessionError::Failed)
                );
                let diagnostic = session.failure_diagnostic().unwrap();
                assert_ne!(diagnostic.source, "thinking_configuration");
                assert_eq!(
                    diagnostic.category,
                    if readback == "timeout" {
                        Category::Timeout
                    } else {
                        Category::Closed
                    }
                );
                spec.config.thinking_option_id = Some("low".into());
                let requests =
                    std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
                assert_eq!(
                    session.start_turn("retry", &spec.config).await,
                    Err(AgentSessionError::Rejected)
                );
                assert_eq!(
                    std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap(),
                    requests
                );
                session.close().await.unwrap();
            }
            let requests =
                std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
            assert!(
                requests
                    .lines()
                    .all(|line| serde_json::from_str::<Value>(line).unwrap()["type"] != "prompt")
            );
        }
    }
}

#[tokio::test]
async fn live_thinking_rejection_blocks_the_same_preference_across_model_changes() {
    let (directory, client, mut spec) = fixture("worded-thinking-rejection");
    std::fs::write(
        directory.path().join("rejection-message"),
        "unsupported level",
    )
    .unwrap();
    let mut session = client.create_session(&spec).await.unwrap();
    spec.config.thinking_option_id = Some("high".into());
    assert_eq!(
        session.start_turn("keep this", &spec.config).await,
        Err(AgentSessionError::Rejected)
    );
    spec.config.model = Some("fixture/other".into());
    assert_eq!(
        session.start_turn("keep this", &spec.config).await,
        Err(AgentSessionError::Rejected)
    );
    assert_eq!(
        session.failure_diagnostic().unwrap().source,
        "thinking_configuration"
    );
    let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    let commands: Vec<Value> = requests
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        commands
            .iter()
            .filter(|command| command["type"] == "set_thinking_level")
            .count(),
        1
    );
    assert!(
        !commands
            .iter()
            .any(|command| command["type"] == "set_model" || command["type"] == "prompt")
    );
    spec.config.thinking_option_id = Some("low".into());
    session.start_turn("corrected", &spec.config).await.unwrap();
    terminal(session.as_mut()).await;
    session.close().await.unwrap();
}

#[tokio::test]
async fn pending_model_only_changes_clamp_the_saved_preference() {
    let (directory, client, mut spec) = fixture("strict-thinking-models");
    spec.config.thinking_option_id = Some("high".into());
    let mut session = client.create_session(&spec).await.unwrap();
    std::fs::write(directory.path().join("control-readback-conflict"), "").unwrap();
    spec.config.model = Some("fixture/other".into());
    assert_eq!(
        session.start_turn("keep this", &spec.config).await,
        Err(AgentSessionError::Rejected)
    );
    assert_eq!(
        session.failure_diagnostic().unwrap().source,
        "thinking_conflict"
    );
    std::fs::remove_file(directory.path().join("control-readback-conflict")).unwrap();
    spec.config.model = Some("fixture/low-ceiling".into());
    session.start_turn("keep this", &spec.config).await.unwrap();
    terminal(session.as_mut()).await;
    assert_eq!(
        native_thinking(&session.runtime_info().await.unwrap()),
        Some("low")
    );
    session.close().await.unwrap();
    let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    let levels: Vec<Value> = requests
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|command| command["type"] == "set_thinking_level")
        .map(|command| command["level"].clone())
        .collect();
    assert_eq!(levels, [json!("high"), json!("high"), json!("low")]);
}
