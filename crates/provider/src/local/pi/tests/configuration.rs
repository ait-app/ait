use super::*;

#[tokio::test]
async fn opening_thinking_rejections_reach_the_durable_failure_notice() {
    use crate::service::agent_manager::{AgentManager, AgentManagerError, AgentRegistration};
    use crate::storage::timeline::Timeline;
    use domain::agent_runtime::registry::AgentRuntimeRegistry;
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
    use provider_core::protocol::agent_config::{ConfigPatch, NullableSetting};

    for resumed in [false, true] {
        let (directory, client, mut spec) = fixture("reject-thinking");
        let registry = FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json"));
        let timeline = Timeline::memory().unwrap();
        let mut manager =
            AgentManager::new(Box::new(registry.clone())).with_timeline(timeline.clone());
        manager.register_client(Box::new(client.clone())).unwrap();
        if !resumed {
            spec.config.thinking_option_id = Some("high".into());
        }
        manager
            .create_with_environment(
                "agent",
                &spec,
                AgentRegistration::default(),
                &client.environment,
            )
            .await
            .unwrap();
        if resumed {
            manager.send("agent", "initial").await.unwrap();
            tokio::time::timeout(Duration::from_secs(5), async {
                while manager.active_turn("agent").is_some() {
                    manager.poll().await.unwrap();
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
            })
            .await
            .unwrap();
            manager.close("agent").await.unwrap();
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
        }

        assert_eq!(
            manager.send("agent", "keep this").await,
            Err(AgentManagerError::Session)
        );

        let record = registry.get("agent").unwrap().unwrap();
        assert_eq!(
            record.last_error.as_deref(),
            Some(
                "Pi: Pi rejected the thinking level. Choose a supported level before retrying. The prompt was not submitted."
            )
        );
        assert_eq!(
            record.config.unwrap().thinking_option_id.as_deref(),
            Some("high")
        );
        let rows = timeline.read("agent").unwrap().1;
        assert!(rows.iter().any(|row| row.entry.item["type"] == "error"
            && row.entry.item["message"].as_str() == record.last_error.as_deref()));
        let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
        let commands: Vec<Value> = requests
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(
            commands
                .iter()
                .filter(|command| command["type"] == "prompt")
                .count(),
            usize::from(resumed)
        );
        assert_eq!(
            commands
                .iter()
                .filter(|command| command["type"] == "set_thinking_level")
                .count(),
            1
        );
        manager.close_all().await.unwrap();
    }
}

#[tokio::test]
async fn correcting_an_opening_rejection_applies_the_new_preference() {
    let (_directory, client, mut spec) = fixture("reject-live-thinking");
    spec.config.thinking_option_id = Some("future".into());
    let mut session = client.create_session(&spec).await.unwrap();
    spec.config.thinking_option_id = Some("low".into());

    session.start_turn("keep this", &spec.config).await.unwrap();

    assert!(matches!(
        terminal(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    assert_eq!(
        native_thinking(&session.runtime_info().await.unwrap()),
        Some("low")
    );
    assert!(session.failure_diagnostic().is_none());
    session.close().await.unwrap();
}

#[tokio::test]
async fn partial_control_rejections_persist_the_actual_runtime_before_settlement() {
    use crate::service::agent_manager::{AgentManager, AgentManagerError, AgentRegistration};
    use domain::agent_runtime::registry::AgentRuntimeRegistry;
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
    use provider_core::protocol::agent_config::{ConfigPatch, NullableSetting};

    let (directory, client, mut spec) = fixture("summary-model-reply-reject-thinking");
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

    assert_eq!(
        manager.send("agent", "keep this").await,
        Err(AgentManagerError::Session)
    );

    let record = registry.get("agent").unwrap().unwrap();
    let info = record.runtime_info.unwrap();
    assert_eq!(info.model.as_deref(), Some("fixture/other"));
    assert_eq!(native_thinking(&info), Some("low"));
    assert_eq!(
        record.config.unwrap().thinking_option_id.as_deref(),
        Some("high")
    );
    assert!(
        record
            .last_error
            .unwrap()
            .contains("Pi rejected the thinking level")
    );
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn thinking_retries_do_not_resend_an_unchanged_model() {
    let (directory, client, mut spec) = fixture("reject-live-thinking");
    spec.config.model = Some("fixture/model".into());
    let mut session = client.create_session(&spec).await.unwrap();
    spec.config.thinking_option_id = Some("future".into());
    for _ in 0..2 {
        assert_eq!(
            session.start_turn("keep this", &spec.config).await,
            Err(AgentSessionError::Rejected)
        );
        assert_eq!(
            native_thinking(&session.runtime_info().await.unwrap()),
            Some("medium")
        );
    }
    spec.config.thinking_option_id = Some("high".into());
    session.start_turn("keep this", &spec.config).await.unwrap();
    terminal(session.as_mut()).await;
    session.close().await.unwrap();

    let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    assert_eq!(
        requests
            .lines()
            .filter(|line| { serde_json::from_str::<Value>(line).unwrap()["type"] == "set_model" })
            .count(),
        1
    );
}

#[tokio::test]
async fn changed_controls_share_one_final_state_barrier() {
    let (directory, client, mut spec) = fixture("multiple-models");
    let mut session = client.create_session(&spec).await.unwrap();
    spec.config.model = Some("fixture/other".into());
    spec.config.thinking_option_id = Some("high".into());

    session.start_turn("hello", &spec.config).await.unwrap();
    terminal(session.as_mut()).await;
    session.close().await.unwrap();

    let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    let commands: Vec<Value> = requests
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let types: Vec<_> = commands
        .iter()
        .map(|command| command["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        types,
        [
            "get_state",
            "get_state",
            "set_model",
            "set_thinking_level",
            "get_state",
            "prompt"
        ]
    );
}

#[tokio::test]
async fn final_control_readback_blocks_work_started_during_configuration() {
    let (directory, client, mut spec) = fixture("control-starts-work");
    let mut session = client.create_session(&spec).await.unwrap();
    spec.config.thinking_option_id = Some("high".into());

    assert_eq!(
        session.start_turn("keep this", &spec.config).await,
        Err(AgentSessionError::Busy)
    );

    let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    assert!(
        requests
            .lines()
            .all(|line| serde_json::from_str::<Value>(line).unwrap()["type"] != "prompt")
    );
    std::fs::write(directory.path().join("native-idle"), "").unwrap();
    session.start_turn("keep this", &spec.config).await.unwrap();
    assert!(matches!(
        terminal(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    session.close().await.unwrap();
}

#[test]
fn thinking_clamping_uses_the_destination_ladder() {
    for (model, level, expected) in [
        (json!({"reasoning":true}), "low", Some("low")),
        (json!({"reasoning":true}), "max", Some("high")),
        (json!({"reasoning":true}), "ultra", Some("off")),
        (
            json!({"reasoning":true,"thinkingLevelMap":{"ultra":"native"}}),
            "ultra",
            Some("off"),
        ),
        (json!({"reasoning":true}), "future", Some("off")),
        (
            json!({"reasoning":true,"thinkingLevelMap":{"medium":null}}),
            "medium",
            Some("high"),
        ),
        (
            json!({"reasoning":true,"thinkingLevelMap":{"medium":null,"high":null}}),
            "medium",
            Some("low"),
        ),
        (json!({"reasoning":false}), "high", Some("off")),
        (Value::Null, "medium", Some("medium")),
        (
            json!({"provider":"fixture","id":"summary"}),
            "high",
            Some("high"),
        ),
        (json!({"reasoning":null}), "medium", Some("medium")),
        (json!({"reasoning":"unknown"}), "future", Some("future")),
    ] {
        assert_eq!(
            config::clamp_thinking_level(&model, level).as_deref(),
            expected
        );
    }
}

#[tokio::test]
async fn model_changes_restore_only_supported_thinking_levels() {
    use std::io::Write;

    for (model, seed, expected) in [
        ("other", "medium", "medium"),
        ("deepseek-flash", "medium", "high"),
        ("low-ceiling", "medium", "low"),
        ("low-ceiling", "high", "low"),
        ("no-reasoning", "high", "off"),
    ] {
        for restored in [false, true] {
            let (directory, client, mut spec) = fixture("strict-thinking-models");
            spec.config.thinking_option_id = Some(seed.into());
            let mut session = client.create_session(&spec).await.unwrap();
            session.start_turn("initial", &spec.config).await.unwrap();
            terminal(session.as_mut()).await;

            if restored {
                let handle = session.persistence().unwrap();
                session.close().await.unwrap();
                let path = handle.native_handle.as_ref().unwrap().as_str().unwrap();
                let mut history = std::fs::OpenOptions::new().append(true).open(path).unwrap();
                writeln!(
                    history,
                    "{}",
                    json!({"type":"thinking_level_change","thinkingLevel":seed})
                )
                .unwrap();
                spec.config.model = Some(format!("fixture/{model}"));
                spec.config.thinking_option_id = None;
                session = client
                    .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
                    .await
                    .unwrap();
            } else {
                spec.config.model = Some(format!("fixture/{model}"));
            }
            session
                .start_turn("after switch", &spec.config)
                .await
                .unwrap();
            assert!(matches!(
                terminal(session.as_mut()).await.last(),
                Some(AgentTurnEvent::Completed(_))
            ));
            let info = session.runtime_info().await.unwrap();
            assert_eq!(info.model.as_deref(), spec.config.model.as_deref());
            assert_eq!(native_thinking(&info), Some(expected));
            session.close().await.unwrap();

            let requests =
                std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
            let commands: Vec<Value> = requests
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .collect();
            let switch = commands
                .iter()
                .position(|command| command["type"] == "set_model")
                .unwrap();
            assert_eq!(commands[switch + 1]["type"], "set_thinking_level");
            assert_eq!(commands[switch + 1]["level"], expected);
            if !restored {
                assert_eq!(spec.config.thinking_option_id.as_deref(), Some(seed));
            }
        }
    }
}

#[tokio::test]
async fn model_round_trips_restore_requested_thinking_after_clamping() {
    for (model, requested, clamped) in [
        ("low-ceiling", "high", "low"),
        ("no-reasoning", "high", "off"),
        ("deepseek-flash", "medium", "high"),
    ] {
        let (directory, client, mut spec) = fixture("strict-thinking-models");
        spec.config.model = Some("fixture/model".into());
        spec.config.thinking_option_id = Some(requested.into());
        let mut session = client.create_session(&spec).await.unwrap();

        for (model, expected) in [(model, clamped), (model, clamped), ("model", requested)] {
            spec.config.model = Some(format!("fixture/{model}"));
            session.start_turn("hello", &spec.config).await.unwrap();
            assert!(matches!(
                terminal(session.as_mut()).await.last(),
                Some(AgentTurnEvent::Completed(_))
            ));
            let info = session.runtime_info().await.unwrap();
            assert_eq!(info.model.as_deref(), spec.config.model.as_deref());
            assert_eq!(
                native_thinking(&info),
                Some(expected),
                "{model}: {requested}"
            );
        }
        session.close().await.unwrap();

        let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
        let levels: Vec<Value> = requests
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .filter(|command| command["type"] == "set_thinking_level")
            .map(|command| command["level"].clone())
            .collect();
        assert_eq!(levels, [json!(requested), json!(clamped), json!(requested)]);
    }
}

#[tokio::test]
async fn summary_model_replies_do_not_skip_thinking_restoration() {
    for (initial, requested, expected) in [
        (None, None, "medium"),
        (Some("high"), Some("high"), "high"),
        (Some("high"), None, "medium"),
    ] {
        let (directory, client, mut spec) = fixture("summary-model-reply");
        spec.config.thinking_option_id = initial.map(str::to_owned);
        let mut session = client.create_session(&spec).await.unwrap();
        spec.config.model = Some("fixture/other".into());
        spec.config.thinking_option_id = requested.map(str::to_owned);

        session.start_turn("hello", &spec.config).await.unwrap();

        assert!(matches!(
            terminal(session.as_mut()).await.last(),
            Some(AgentTurnEvent::Completed(_))
        ));
        assert_eq!(
            native_thinking(&session.runtime_info().await.unwrap()),
            Some(expected)
        );
        session.close().await.unwrap();

        let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
        let commands: Vec<Value> = requests
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect();
        let switch = commands
            .iter()
            .position(|command| command["type"] == "set_model")
            .unwrap();
        assert_eq!(commands[switch + 1]["type"], "set_thinking_level");
        assert_eq!(commands[switch + 1]["level"], expected);
    }
}

#[tokio::test]
async fn summary_model_reply_thinking_rejections_require_a_corrected_preference() {
    let (directory, client, mut spec) = fixture("summary-model-reply-reject-thinking");
    spec.config.thinking_option_id = Some("high".into());
    let mut session = client.create_session(&spec).await.unwrap();
    spec.config.model = Some("fixture/other".into());

    assert_eq!(
        session.start_turn("keep this", &spec.config).await,
        Err(AgentSessionError::Rejected)
    );
    assert_eq!(
        session.failure_diagnostic().unwrap().source,
        "thinking_configuration"
    );
    let info = session.runtime_info().await.unwrap();
    assert_eq!(info.model.as_deref(), Some("fixture/other"));
    assert_eq!(native_thinking(&info), Some("low"));
    let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    assert!(
        requests
            .lines()
            .all(|line| serde_json::from_str::<Value>(line).unwrap()["type"] != "prompt")
    );

    std::fs::write(directory.path().join("accept-thinking"), "").unwrap();
    assert_eq!(
        session.start_turn("keep this", &spec.config).await,
        Err(AgentSessionError::Rejected)
    );
    spec.config.thinking_option_id = Some("low".into());
    session.start_turn("keep this", &spec.config).await.unwrap();

    assert!(matches!(
        terminal(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    assert_eq!(
        native_thinking(&session.runtime_info().await.unwrap()),
        Some("low")
    );
    session.close().await.unwrap();

    let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    assert_eq!(
        requests
            .lines()
            .filter(|line| { serde_json::from_str::<Value>(line).unwrap()["type"] == "set_model" })
            .count(),
        1
    );
}

#[tokio::test]
async fn opening_clamps_native_thinking_to_the_selected_model() {
    for (model, expected) in [
        ("deepseek-flash", "high"),
        ("low-ceiling", "low"),
        ("no-reasoning", "off"),
    ] {
        let (_directory, client, mut spec) = fixture("strict-thinking-models");
        spec.config.model = Some(format!("fixture/{model}"));
        let mut session = client.create_session(&spec).await.unwrap();
        assert_eq!(
            native_thinking(&session.runtime_info().await.unwrap()),
            Some(expected)
        );
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn opening_and_resume_clamp_explicit_preferences_without_replacing_the_requested_seed() {
    for (model, requested, expected) in [
        ("model", "xhigh", "high"),
        ("deepseek-flash", "medium", "high"),
        ("low-ceiling", "xhigh", "low"),
        ("no-reasoning", "high", "off"),
    ] {
        let (directory, client, mut spec) = fixture("strict-thinking-models");
        spec.config.model = Some(format!("fixture/{model}"));
        spec.config.thinking_option_id = Some(requested.into());
        let mut session = client.create_session(&spec).await.unwrap();
        for resumed in [false, true] {
            if resumed {
                let handle = session.persistence().unwrap();
                session.close().await.unwrap();
                session = client
                    .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
                    .await
                    .unwrap();
            }
            assert_eq!(
                native_thinking(&session.runtime_info().await.unwrap()),
                Some(expected)
            );
            session
                .start_turn("first prompt", &spec.config)
                .await
                .unwrap();
            assert!(matches!(
                terminal(session.as_mut()).await.last(),
                Some(AgentTurnEvent::Completed(_))
            ));
            assert_eq!(spec.config.thinking_option_id.as_deref(), Some(requested));
        }
        session.close().await.unwrap();
        let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
        let levels: Vec<Value> = requests
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .filter(|command| command["type"] == "set_thinking_level")
            .map(|command| command["level"].clone())
            .collect();
        assert_eq!(levels, [json!(expected), json!(expected)]);
    }
}

#[tokio::test]
async fn rejected_model_control_can_be_corrected_on_the_same_writer() {
    let (_directory, client, mut spec) = fixture("");
    let mut session = client.create_session(&spec).await.unwrap();
    spec.config.model = Some("fixture/missing".into());

    assert_eq!(
        session.start_turn("preserve me", &spec.config).await,
        Err(AgentSessionError::Rejected)
    );

    spec.config.model = Some("fixture/model".into());
    session
        .start_turn("preserve me", &spec.config)
        .await
        .unwrap();
    assert!(matches!(
        terminal(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn rejected_live_thinking_keeps_the_writer_and_reports_the_control() {
    let (_directory, client, mut spec) = fixture("reject-live-thinking");
    let mut session = client.create_session(&spec).await.unwrap();
    let identity = session.persistence().unwrap().session_id;
    spec.config.thinking_option_id = Some("future".into());

    assert_eq!(
        session.start_turn("keep this", &spec.config).await,
        Err(AgentSessionError::Rejected)
    );
    assert_eq!(
        session.failure_diagnostic().unwrap().source,
        "thinking_configuration"
    );
    assert!(
        session
            .failure_message()
            .unwrap()
            .contains("thinking level")
    );
    spec.config.thinking_option_id = Some("low".into());
    session.start_turn("keep this", &spec.config).await.unwrap();
    assert!(matches!(
        terminal(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    assert_eq!(session.persistence().unwrap().session_id, identity);
    assert!(session.failure_diagnostic().is_none());
    session.close().await.unwrap();
}

#[tokio::test]
async fn native_work_blocks_model_and_thinking_mutations_before_admission() {
    for scenario in ["native-streaming", "native-compacting"] {
        let (directory, client, mut spec) = fixture(scenario);
        let mut session = client.create_session(&spec).await.unwrap();
        spec.config.model = Some("fixture/model".into());
        spec.config.thinking_option_id = Some("high".into());

        assert_eq!(
            session.start_turn("keep this", &spec.config).await,
            Err(AgentSessionError::Busy)
        );
        let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
        assert!(
            requests
                .lines()
                .all(|line| serde_json::from_str::<Value>(line).unwrap()["type"] == "get_state")
        );

        std::fs::write(directory.path().join("native-idle"), "").unwrap();
        session.start_turn("keep this", &spec.config).await.unwrap();
        assert!(matches!(
            terminal(session.as_mut()).await.last(),
            Some(AgentTurnEvent::Completed(_))
        ));
        assert_eq!(
            native_thinking(&session.runtime_info().await.unwrap()),
            Some("high")
        );
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn clearing_thinking_uses_destination_model_capabilities() {
    for (model, selected, expected) in [
        ("model", "high", "medium"),
        ("deepseek-flash", "high", "high"),
        ("low-ceiling", "low", "low"),
        ("no-reasoning", "off", "off"),
    ] {
        for combined in [false, true] {
            let (_directory, client, mut spec) = fixture("strict-thinking-models");
            spec.config.model = Some(format!(
                "fixture/{}",
                if combined { "model" } else { model }
            ));
            spec.config.thinking_option_id = Some(selected.into());
            let mut session = client.create_session(&spec).await.unwrap();
            spec.config.model = Some(format!("fixture/{model}"));
            spec.config.thinking_option_id = None;

            session.start_turn("hello", &spec.config).await.unwrap();

            assert!(matches!(
                terminal(session.as_mut()).await.last(),
                Some(AgentTurnEvent::Completed(_))
            ));
            assert_eq!(
                native_thinking(&session.runtime_info().await.unwrap()),
                Some(expected)
            );
            session.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn reverting_preferences_after_a_partial_control_rejection_restores_the_model() {
    let (_directory, client, mut spec) = fixture("strict-thinking-models");
    spec.config.model = Some("fixture/model".into());
    spec.config.thinking_option_id = Some("medium".into());
    let mut session = client.create_session(&spec).await.unwrap();
    let mut rejected = spec.config.clone();
    rejected.model = Some("fixture/deepseek-flash".into());
    rejected.thinking_option_id = Some("future".into());
    assert_eq!(
        session.start_turn("hello", &rejected).await,
        Err(AgentSessionError::Rejected)
    );

    session.start_turn("hello", &spec.config).await.unwrap();

    assert!(matches!(
        terminal(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    let info = session.runtime_info().await.unwrap();
    assert_eq!(info.model.as_deref(), Some("fixture/model"));
    assert_eq!(native_thinking(&info), Some("medium"));
    session.close().await.unwrap();
}
