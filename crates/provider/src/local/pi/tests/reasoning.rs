use super::*;

#[test]
fn model_thinking_options_follow_native_capabilities() {
    for (reasoning, mapping, expected) in [
        (false, Value::Null, vec![]),
        (
            true,
            Value::Null,
            vec!["off", "minimal", "low", "medium", "high"],
        ),
        (
            true,
            json!({"ultra":"native","future":null,"":"native","bad\nlevel":"native"}),
            vec!["off", "minimal", "low", "medium", "high", "ultra"],
        ),
        (false, json!({"ultra":"native"}), vec![]),
        (
            true,
            json!({"off":null,"minimal":null,"medium":null,"high":null,"xhigh":"native","max":42}),
            vec!["low", "xhigh", "max"],
        ),
        (
            true,
            json!({"medium":null,"high":null,"xhigh":null,"max":null}),
            vec!["off", "minimal", "low"],
        ),
        (
            true,
            json!({"off":null,"minimal":null,"low":null,"medium":null,"high":null}),
            vec![],
        ),
        (
            true,
            json!({"off":null,"minimal":null,"low":null,"medium":null,"high":null,"xhigh":null,"max":null}),
            vec![],
        ),
    ] {
        let models = config::models(
            &json!({"models":[{
            "provider":"backend","id":"reasoner","reasoning":reasoning,"thinkingLevelMap":mapping
        }, {"provider":"other","id":"reasoner"}]}),
            &json!({"model":{"provider":"backend","id":"reasoner"}}),
        )
        .unwrap();
        let options = models[0]["thinkingOptions"].as_array().unwrap();
        assert_eq!(
            options
                .iter()
                .map(|option| option["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            expected
        );
        let expected_default = expected
            .iter()
            .find(|id| matches!(**id, "medium" | "high" | "xhigh" | "max"))
            .or_else(|| expected.last())
            .copied();
        assert_eq!(
            models[0]["defaultThinkingOptionId"].as_str(),
            expected_default
        );
        assert_eq!(models[0]["isDefault"], true);
        assert_eq!(models[1]["isDefault"], false);
        assert_eq!(models[1]["thinkingOptions"], json!([]));
    }
}

#[test]
fn catalog_defaults_clamp_medium_for_each_model_independently_of_session_settings() {
    for (mapping, expected) in [
        (json!({}), "medium"),
        (json!({"medium":null}), "high"),
        (json!({"medium":null,"high":null,"xhigh":"high"}), "xhigh"),
        (json!({"medium":null,"high":null}), "low"),
        (json!({"medium":null,"high":null,"ultra":"high"}), "low"),
        (
            json!({"low":null,"medium":null,"high":null,"ultra":"high"}),
            "minimal",
        ),
        (
            json!({"minimal":null,"low":null,"medium":null,"high":null,"ultra":"high"}),
            "off",
        ),
    ] {
        let models = config::models(
            &json!({"models":[
                {"provider":"backend","id":"a","reasoning":true,"thinkingLevelMap":mapping},
                {"provider":"backend","id":"b","reasoning":true,"thinkingLevelMap":mapping}
            ]}),
            &json!({"model":{"provider":"backend","id":"a"},"thinkingLevel":"off"}),
        )
        .unwrap();
        for model in models {
            assert_eq!(model["defaultThinkingOptionId"], expected);
            let defaults: Vec<_> = model["thinkingOptions"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|option| option["isDefault"] == true)
                .collect();
            assert_eq!(defaults.len(), 1);
            assert_eq!(defaults[0]["id"], expected);
        }
    }
}

#[tokio::test]
async fn selections_allow_native_clamping_with_explicit_or_default_models() {
    for (scenario, level, valid) in [
        ("", "high", true),
        ("", "off", true),
        ("", "xhigh", true),
        ("mapped-thinking", "medium", true),
        ("mapped-thinking", "xhigh", true),
        ("mapped-thinking", "max", true),
        ("no-reasoning", "high", true),
        ("", "future", true),
        ("", "ultra", true),
        ("", "", false),
        ("", "bad\nlevel", false),
    ] {
        let (_directory, client, mut spec) = fixture(scenario);
        spec.config.thinking_option_id = Some(level.into());
        assert_eq!(
            client.validate_selection(&spec).await.is_ok(),
            valid,
            "{scenario}: {level}"
        );
        spec.config.model = Some("fixture/model".into());
        assert_eq!(
            client.validate_selection(&spec).await.is_ok(),
            valid,
            "{scenario}: {level}"
        );
    }
}

#[test]
fn thinking_ids_are_bounded_without_restricting_native_vocabulary() {
    for (level, valid) in [
        ("ultra".to_owned(), true),
        ("future-level".to_owned(), true),
        ("a".repeat(512), true),
        ("a".repeat(513), false),
        (String::new(), false),
        ("bad\0level".to_owned(), false),
    ] {
        let config = StoredAgentConfig {
            thinking_option_id: Some(level.clone()),
            ..StoredAgentConfig::default()
        };
        assert_eq!(config::validate(&config).is_ok(), valid, "{level:?}");
    }
}

#[test]
fn catalog_preserves_future_native_options_without_guessing_their_default() {
    let models = config::models(
        &json!({"models":[{"provider":"backend","id":"reasoner","reasoning":true,
            "thinkingLevelMap":{"ultra":"native","future":null}}]}),
        &json!({"model":{"provider":"backend","id":"reasoner"},"thinkingLevel":"ultra"}),
    )
    .unwrap();

    assert_eq!(models[0]["defaultThinkingOptionId"], "medium");
    assert_eq!(
        models[0]["thinkingOptions"].as_array().unwrap().last(),
        Some(&json!({"id":"ultra","label":"ultra"}))
    );
}

#[tokio::test]
async fn clamped_thinking_selections_survive_updates_restart_and_import() {
    use crate::protocol::agent_config::{ConfigPatch, NullableSetting};
    use crate::protocol::resume::Overrides;
    use crate::service::agent_manager::{AgentManager, AgentRegistration};
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

    for level in ["max", "ultra"] {
        let (directory, client, mut spec) = fixture("clamp-thinking");
        spec.config.model = Some("fixture/model".into());
        spec.config.thinking_option_id = Some(level.into());
        let details = client.discover(&spec.cwd).await.unwrap();
        assert!(
            details.models[0]["thinkingOptions"]
                .as_array()
                .unwrap()
                .iter()
                .all(|option| option["id"] != level)
        );
        let registry = FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json"));
        let mut manager = AgentManager::new(Box::new(registry.clone()));
        manager.register_client(Box::new(client.clone())).unwrap();
        let created = manager
            .create_with_environment(
                "clamped",
                &spec,
                AgentRegistration::default(),
                &client.environment,
            )
            .await
            .unwrap();
        assert_eq!(
            created
                .config
                .as_ref()
                .unwrap()
                .thinking_option_id
                .as_deref(),
            Some("high")
        );
        assert_eq!(
            crate::service::agent_runtime::effective_thinking_option_id(&created),
            Some("high")
        );
        assert_eq!(
            native_thinking(&created.runtime_info.unwrap()),
            Some("high")
        );
        let patch = ConfigPatch {
            mode_id: NullableSetting::Set("default".into()),
            ..ConfigPatch::default()
        };
        manager.configure("clamped", &patch).await.unwrap();
        manager.close_all().await.unwrap();
        drop(manager);

        let mut restored = AgentManager::new(Box::new(registry));
        restored.register_client(Box::new(client.clone())).unwrap();
        let record = restored
            .restore("clamped", &Overrides::default())
            .await
            .unwrap();
        assert_eq!(
            record.config.unwrap().thinking_option_id.as_deref(),
            Some("high")
        );
        assert_eq!(native_thinking(&record.runtime_info.unwrap()), Some("high"));
        restored.configure("clamped", &patch).await.unwrap();
        let mut imported = restored.live_snapshot("clamped").unwrap().clone();
        imported.id = "imported".into();
        restored.close_all().await.unwrap();
        let mut manager = AgentManager::new(Box::new(FileBackedAgentRuntimeRegistry::new(
            directory.path().join("imported-agents.json"),
        )));
        manager.register_client(Box::new(client)).unwrap();
        let record = manager.restore_new(imported).await.unwrap();
        assert_eq!(native_thinking(&record.runtime_info.unwrap()), Some("high"));
        manager.close_all().await.unwrap();
    }
}

#[tokio::test]
async fn thinking_selection_reads_effective_level_and_preserves_native_default() {
    for (scenario, selection, expected) in [
        ("mapped-thinking", None, "high"),
        ("", Some("off"), "off"),
        ("", Some("max"), "max"),
        ("", Some("ultra"), "ultra"),
        ("clamp-thinking", Some("medium"), "high"),
    ] {
        let (directory, client, mut spec) = fixture(scenario);
        spec.config.thinking_option_id = selection.map(str::to_owned);
        let mut session = client.create_session(&spec).await.unwrap();
        assert_eq!(
            session
                .runtime_info()
                .await
                .unwrap()
                .thinking_option_id
                .as_deref(),
            Some(expected)
        );
        assert_eq!(
            native_thinking(&session.runtime_info().await.unwrap()),
            Some(expected)
        );
        session.close().await.unwrap();
        let log = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
        let selections: Vec<Value> = log
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .filter(|command: &Value| command["type"] == "set_thinking_level")
            .collect();
        assert_eq!(selections.len(), usize::from(selection.is_some()));
        if let Some(level) = selection {
            assert_eq!(selections[0]["level"], level);
        }
    }
}

#[tokio::test]
async fn thinking_changes_apply_between_turns_without_resetting_an_unspecified_level() {
    let (_directory, client, mut spec) = fixture("");
    let mut session = client.create_session(&spec).await.unwrap();
    for (selection, expected) in [(Some("high"), "high"), (Some("low"), "low"), (None, "low")] {
        spec.config.thinking_option_id = selection.map(str::to_owned);
        session.start_turn("hello", &spec.config).await.unwrap();
        terminal(session.as_mut()).await;
        assert_eq!(
            session
                .runtime_info()
                .await
                .unwrap()
                .thinking_option_id
                .as_deref(),
            Some(expected)
        );
        assert_eq!(
            native_thinking(&session.runtime_info().await.unwrap()),
            Some(expected)
        );
    }
    session.close().await.unwrap();
}

#[tokio::test]
async fn unrelated_config_changes_preserve_extension_thinking_until_host_selection_changes() {
    let (directory, client, mut spec) = fixture("multiple-models");
    spec.config.model = Some("fixture/model".into());
    spec.config.thinking_option_id = Some("high".into());
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("thinking-after", &spec.config)
        .await
        .unwrap();
    terminal(session.as_mut()).await;

    for (model, mode) in [
        ("fixture/other", None),
        ("fixture/other", Some("default")),
        ("fixture/model", Some("default")),
    ] {
        spec.config.model = Some(model.into());
        spec.config.mode_id = mode.map(str::to_owned);
        session.start_turn("hello", &spec.config).await.unwrap();
        terminal(session.as_mut()).await;
        let info = session.runtime_info().await.unwrap();
        assert_eq!(info.model.as_deref(), Some(model));
        assert_eq!(
            native_thinking(&info),
            Some(if model == "fixture/other" {
                "low"
            } else {
                "medium"
            })
        );
    }
    for (selection, expected) in [
        (Some("minimal"), "minimal"),
        (Some("high"), "high"),
        (None, "high"),
    ] {
        spec.config.thinking_option_id = selection.map(str::to_owned);
        session.start_turn("hello", &spec.config).await.unwrap();
        terminal(session.as_mut()).await;
        assert_eq!(
            native_thinking(&session.runtime_info().await.unwrap()),
            Some(expected)
        );
    }
    session.close().await.unwrap();

    let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    let controls: Vec<Value> = requests
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|command| {
            matches!(
                command["type"].as_str(),
                Some("set_model" | "set_thinking_level")
            )
        })
        .collect();
    let levels: Vec<_> = controls
        .iter()
        .filter(|command| command["type"] == "set_thinking_level")
        .map(|command| command["level"].as_str().unwrap())
        .collect();
    assert_eq!(levels, ["high", "minimal", "high"]);
    let models: Vec<_> = controls
        .iter()
        .filter(|command| command["type"] == "set_model")
        .map(|command| command["modelId"].as_str().unwrap())
        .collect();
    assert_eq!(models, ["other", "model"]);
    assert_eq!(controls[0]["type"], "set_thinking_level");
    assert_eq!(controls[1]["type"], "set_model");
}

#[tokio::test]
async fn resume_reapplies_explicit_thinking_after_an_extension_change() {
    let (_directory, client, mut spec) = fixture("");
    spec.config.thinking_option_id = Some("high".into());
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("thinking-after", &spec.config)
        .await
        .unwrap();
    terminal(session.as_mut()).await;
    assert_eq!(
        native_thinking(&session.runtime_info().await.unwrap()),
        Some("low")
    );
    // Materialize native history before exercising interactive resume.
    session.start_turn("hello", &spec.config).await.unwrap();
    terminal(session.as_mut()).await;
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();

    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();

    assert_eq!(
        native_thinking(&resumed.runtime_info().await.unwrap()),
        Some("high")
    );
    resumed.close().await.unwrap();
}

#[tokio::test]
async fn extension_thinking_changes_are_reported_during_admission_and_polling() {
    for command in ["thinking-before", "thinking-after", "thinking-handled"] {
        let (_directory, client, mut spec) = fixture("");
        spec.config.thinking_option_id = Some("high".into());
        let mut session = client.create_session(&spec).await.unwrap();
        session.start_turn(command, &spec.config).await.unwrap();
        let events = terminal(session.as_mut()).await;
        assert_eq!(
            session
                .runtime_info()
                .await
                .unwrap()
                .thinking_option_id
                .as_deref(),
            Some("low")
        );
        assert!(events.iter().any(|event| matches!(event, AgentTurnEvent::RuntimeInfo(info) if native_thinking(info) == Some("low"))), "{command}: {events:?}");
        session.start_turn("hello", &spec.config).await.unwrap();
        terminal(session.as_mut()).await;
        assert_eq!(
            native_thinking(&session.runtime_info().await.unwrap()),
            Some("low")
        );
        session.close().await.unwrap();
    }
    let (_directory, client, spec) = fixture("idle-thinking-change");
    let mut session = client.create_session(&spec).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Some(AgentTurnEvent::RuntimeInfo(info)) = session.poll_turn().unwrap() {
                assert_eq!(native_thinking(&info), Some("low"));
                assert_eq!(info.thinking_option_id.as_deref(), Some("low"));
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    session.close().await.unwrap();
}

#[tokio::test]
async fn native_thinking_state_is_optional_and_forward_compatible() {
    for (scenario, expected) in [
        ("bad-thinking-state", Some("future")),
        ("none-thinking-state", Some("none")),
        ("ultra-thinking-state", Some("ultra")),
        ("missing-thinking-state", None),
        ("null-thinking-state", None),
        ("malformed-thinking-state", None),
    ] {
        let (_directory, client, spec) = fixture(scenario);
        let mut session = client.create_session(&spec).await.unwrap();
        for _ in 0..2 {
            assert_eq!(
                native_thinking(&session.runtime_info().await.unwrap()),
                expected,
                "{scenario}"
            );
            session.start_turn("hello", &spec.config).await.unwrap();
            assert!(matches!(
                terminal(session.as_mut()).await.last(),
                Some(AgentTurnEvent::Completed(_))
            ));
        }
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn missing_thinking_state_clears_previous_runtime_level() {
    let (_directory, client, spec) = fixture("thinking-state-disappears");
    let mut session = client.create_session(&spec).await.unwrap();
    assert_eq!(
        session
            .effective_config()
            .unwrap()
            .thinking_option_id
            .as_deref(),
        None
    );
    session.start_turn("hello", &spec.config).await.unwrap();
    let events = terminal(session.as_mut()).await;
    assert!(events.iter().any(|event| matches!(event,
        AgentTurnEvent::RuntimeInfo(info) if info.extra.is_none())));
    assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
    assert!(session.runtime_info().await.unwrap().extra.is_none());
    session.close().await.unwrap();
}

#[tokio::test]
async fn native_thinking_events_do_not_fail_admission_or_turns() {
    for (scenario, expected) in [
        ("thinking-event-future", "future"),
        ("thinking-event-none", "none"),
        ("thinking-event-ultra", "ultra"),
        ("thinking-event-missing", "medium"),
        ("thinking-event-null", "medium"),
        ("thinking-event-malformed", "medium"),
    ] {
        for command in ["thinking-before", "thinking-after", "thinking-handled"] {
            let (_directory, client, spec) = fixture(scenario);
            let mut session = client.create_session(&spec).await.unwrap();
            session.start_turn(command, &spec.config).await.unwrap();
            let events = terminal(session.as_mut()).await;
            assert!(
                matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
                "{scenario}: {command}: {events:?}"
            );
            assert_eq!(
                native_thinking(&session.runtime_info().await.unwrap()),
                Some(expected),
                "{scenario}: {command}"
            );
            session.start_turn("hello", &spec.config).await.unwrap();
            assert!(matches!(
                terminal(session.as_mut()).await.last(),
                Some(AgentTurnEvent::Completed(_))
            ));
            session.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn native_thinking_events_do_not_close_idle_sessions() {
    for (scenario, expected) in [
        ("idle-thinking-future", "future"),
        ("idle-thinking-missing", "low"),
        ("idle-thinking-malformed", "low"),
    ] {
        let (_directory, client, spec) = fixture(scenario);
        let mut session = client.create_session(&spec).await.unwrap();
        // The fixture queues the idle event before its get_state response.
        if let Some(event) = session.poll_turn().unwrap() {
            assert!(matches!(event, AgentTurnEvent::RuntimeInfo(_)), "{event:?}");
        }
        assert_eq!(
            native_thinking(&session.runtime_info().await.unwrap()),
            Some(expected),
            "{scenario}"
        );
        session.start_turn("hello", &spec.config).await.unwrap();
        assert!(matches!(
            terminal(session.as_mut()).await.last(),
            Some(AgentTurnEvent::Completed(_))
        ));
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejected_thinking_controls_fail_explicitly() {
    let (_directory, client, mut spec) = fixture("reject-thinking");
    spec.config.thinking_option_id = Some("high".into());
    assert!(client.create_session(&spec).await.is_err());
}

#[tokio::test]
async fn idle_thinking_updates_use_latest_selection_for_directory_identity() {
    use crate::protocol::agent_config::{ConfigPatch, NullableSetting};
    use crate::service::agent_manager::{AgentManager, AgentRegistration};
    use crate::service::agent_runtime::effective_thinking_option_id;
    use domain::agent_runtime::registry::AgentRuntimeRegistry;
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

    let (directory, client, mut spec) = fixture("");
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

    for selection in [Some("low"), Some("high"), None] {
        manager
            .configure(
                "agent",
                &ConfigPatch {
                    thinking_option_id: selection.map_or(NullableSetting::Clear, |level| {
                        NullableSetting::Set(level.into())
                    }),
                    ..ConfigPatch::default()
                },
            )
            .await
            .unwrap();

        for poll in [false, true] {
            if poll {
                manager.poll().await.unwrap();
            }
            let record = registry.get("agent").unwrap().unwrap();
            assert_eq!(
                record
                    .config
                    .as_ref()
                    .unwrap()
                    .thinking_option_id
                    .as_deref(),
                selection
            );
            assert_eq!(
                effective_thinking_option_id(&record),
                Some(selection.unwrap_or("medium"))
            );
            assert_eq!(
                native_thinking(record.runtime_info.as_ref().unwrap()),
                Some(selection.unwrap_or("medium"))
            );
        }
    }
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn runtime_readback_updates_saved_thinking_and_filter_identity() {
    use crate::service::agent_manager::{AgentManager, AgentRegistration};
    use crate::service::agent_runtime::effective_thinking_option_id;
    use domain::agent_runtime::registry::AgentRuntimeRegistry;
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

    for selection in [None, Some("max")] {
        let (directory, client, mut spec) = fixture("clamp-thinking");
        spec.config.thinking_option_id = selection.map(str::to_owned);
        let registry = FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json"));
        let mut manager = AgentManager::new(Box::new(registry.clone()));
        manager.register_client(Box::new(client.clone())).unwrap();
        let record = manager
            .create_with_environment(
                "agent",
                &spec,
                AgentRegistration::default(),
                &client.environment,
            )
            .await
            .unwrap();
        assert_eq!(
            effective_thinking_option_id(&record),
            native_thinking(record.runtime_info.as_ref().unwrap())
        );
        manager.send("agent", "thinking-after").await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                manager.poll().await.unwrap();
                let record = registry.get("agent").unwrap().unwrap();
                assert_eq!(
                    effective_thinking_option_id(&record),
                    native_thinking(record.runtime_info.as_ref().unwrap())
                );
                if manager.active_turn("agent").is_none() {
                    assert_eq!(
                        native_thinking(record.runtime_info.as_ref().unwrap()),
                        Some("low")
                    );
                    break;
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
        manager.close_all().await.unwrap();
    }
}

#[tokio::test]
async fn model_only_updates_use_destination_defaults() {
    for selection in [None, Some("high")] {
        let (_directory, client, mut spec) = fixture("multiple-models");
        spec.config.model = Some("fixture/model".into());
        spec.config.thinking_option_id = selection.map(str::to_owned);
        let mut session = client.create_session(&spec).await.unwrap();
        spec.config.model = Some("fixture/other".into());
        session.start_turn("hello", &spec.config).await.unwrap();
        terminal(session.as_mut()).await;
        assert_eq!(
            native_thinking(&session.runtime_info().await.unwrap()),
            Some("low")
        );
        spec.config.model = Some("fixture/model".into());
        spec.config.thinking_option_id = Some("minimal".into());
        session.start_turn("hello", &spec.config).await.unwrap();
        terminal(session.as_mut()).await;
        assert_eq!(
            native_thinking(&session.runtime_info().await.unwrap()),
            Some("minimal")
        );
        spec.config.model = Some("fixture/clamped".into());
        session.start_turn("hello", &spec.config).await.unwrap();
        terminal(session.as_mut()).await;
        assert_eq!(
            native_thinking(&session.runtime_info().await.unwrap()),
            Some("medium")
        );
        session.close().await.unwrap();
    }
}
