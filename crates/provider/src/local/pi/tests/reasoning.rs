use super::*;

#[test]
fn native_model_identity_requires_only_provider_and_id_strings() {
    for (model, expected) in [
        (
            json!({"provider":"backend","id":"model"}),
            Some(("backend", "model")),
        ),
        (
            json!({"provider":"other","id":"model","name":"Changed"}),
            Some(("other", "model")),
        ),
        (
            json!({"provider":"backend","id":"other"}),
            Some(("backend", "other")),
        ),
        (json!({"provider":"backend"}), None),
        (json!({"id":"model"}), None),
        (json!({"provider":42,"id":"model"}), None),
        (json!({"provider":"backend","id":false}), None),
        (Value::Null, None),
    ] {
        assert_eq!(config::model_identity(&model), expected);
    }
}

#[test]
fn thinking_clamping_has_no_baseline_when_known_model_options_exclude_every_level() {
    assert_eq!(
        config::clamp_thinking_level(
            &json!({"reasoning":true,
                "thinkingLevelMap":{"off":null,"minimal":null,"low":null,"medium":null,"high":null}
            }),
            "medium"
        ),
        None
    );
}

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
            vec!["off", "minimal", "low", "medium", "high"],
        ),
        (false, json!({"ultra":"native"}), vec![]),
        (
            true,
            json!({"high":"ultra","xhigh":"native","max":"native"}),
            vec!["off", "minimal", "low", "medium", "high", "xhigh", "max"],
        ),
        (
            true,
            json!({"off":null,"minimal":null,"medium":null,"high":null,"xhigh":"native","max":42}),
            vec!["low", "xhigh", "max"],
        ),
        (
            true,
            json!({"medium":null,"high":null,"xhigh":null,"max":null,"ultra":null}),
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
        assert!(models[0].get("defaultThinkingOptionId").is_none());
        assert!(
            options
                .iter()
                .all(|option| option.get("isDefault").is_none())
        );
        assert_eq!(models[0]["isDefault"], true);
        assert_eq!(models[1]["isDefault"], false);
        assert_eq!(models[1]["thinkingOptions"], json!([]));
    }
}

#[test]
fn catalog_thinking_defaults_clamp_ambient_state_for_every_model() {
    let data = json!({"models":[
        {"provider":"backend","id":"reasoner","reasoning":true},
        {"provider":"other","id":"reasoner","reasoning":true},
        {"provider":"backend","id":"high-only","reasoning":true,
            "thinkingLevelMap":{"off":null,"minimal":null,"low":null,"medium":null}},
        {"provider":"backend","id":"low-ceiling","reasoning":true,
            "thinkingLevelMap":{"medium":null,"high":null}},
        {"provider":"backend","id":"no-reasoning","reasoning":false},
        {"provider":"backend","id":"no-options","reasoning":true,
            "thinkingLevelMap":{"off":null,"minimal":null,"low":null,"medium":null,"high":null}}
    ]});
    for (level, expected, high_only, low_ceiling) in [
        (json!("medium"), Some("medium"), Some("high"), Some("low")),
        (json!("low"), Some("low"), Some("high"), Some("low")),
        (json!("off"), Some("off"), Some("high"), Some("off")),
        (json!("future"), Some("off"), Some("high"), Some("off")),
        (json!("xhigh"), Some("high"), Some("high"), Some("low")),
        (json!("max"), Some("high"), Some("high"), Some("low")),
        (json!("ultra"), Some("off"), Some("high"), Some("off")),
        (json!(""), None, None, None),
        (json!("bad\nlevel"), None, None, None),
        (json!("a".repeat(513)), None, None, None),
        (Value::Null, None, None, None),
        (json!(42), None, None, None),
    ] {
        let models = config::models(
            &data,
            &json!({"model":{"provider":"backend","id":"reasoner"},"thinkingLevel":level}),
        )
        .unwrap();
        for (model, expected) in
            models
                .iter()
                .zip([expected, expected, high_only, low_ceiling, None, None])
        {
            assert_eq!(model["defaultThinkingOptionId"].as_str(), expected);
            assert_eq!(
                model["thinkingOptions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|option| option["isDefault"] == true)
                    .map(|option| option["id"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                expected.into_iter().collect::<Vec<_>>()
            );
        }
        assert_eq!(models[0]["isDefault"], true);
        assert!(models[1..].iter().all(|model| model["isDefault"] == false));
        assert_eq!(
            models[0]["thinkingOptions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|option| option["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["off", "minimal", "low", "medium", "high"]
        );
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

#[tokio::test]
async fn native_thinking_snapshots_are_bounded_before_publication_or_reapplication() {
    for (name, expected) in [
        ("boundary", Some("a".repeat(512))),
        ("multibyte-boundary", Some("é".repeat(256))),
        ("oversized", None),
        ("large", None),
        ("multibyte-oversized", None),
        ("control", None),
        ("nul", None),
        ("unicode-control", None),
        ("empty", None),
    ] {
        let (directory, client, mut spec) = fixture(&format!("{name}-thinking-state"));
        let mut session = client.create_session(&spec).await.unwrap();
        assert_eq!(
            native_thinking(&session.runtime_info().await.unwrap()),
            expected.as_deref(),
            "{name}"
        );

        spec.config.model = Some("fixture/other".into());
        session.start_turn("hello", &spec.config).await.unwrap();
        terminal(session.as_mut()).await;
        assert_eq!(
            native_thinking(&session.runtime_info().await.unwrap()),
            expected.as_deref(),
            "{name}"
        );
        session.close().await.unwrap();

        let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
        let levels: Vec<Value> = requests
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .filter(|command| command["type"] == "set_thinking_level")
            .map(|command| command["level"].clone())
            .collect();
        assert_eq!(
            levels,
            expected
                .into_iter()
                .map(|_| json!("off"))
                .collect::<Vec<_>>(),
            "{name}"
        );
    }
}

#[tokio::test]
async fn native_thinking_events_share_the_host_metadata_bounds() {
    for (name, valid) in [
        ("boundary", Some("a".repeat(512))),
        ("multibyte-boundary", Some("é".repeat(256))),
        ("oversized", None),
        ("large", None),
        ("multibyte-oversized", None),
        ("control", None),
        ("nul", None),
        ("unicode-control", None),
        ("empty", None),
    ] {
        for command in ["thinking-before", "thinking-after", "thinking-handled"] {
            let (_directory, client, spec) = fixture(&format!("thinking-event-{name}"));
            let mut session = client.create_session(&spec).await.unwrap();
            session.start_turn(command, &spec.config).await.unwrap();
            let events = terminal(session.as_mut()).await;
            let expected = valid
                .as_deref()
                .or((command != "thinking-handled").then_some("medium"));
            assert_eq!(
                native_thinking(&session.runtime_info().await.unwrap()),
                expected,
                "{name}: {command}"
            );
            assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
            let levels: Vec<_> = events
                .iter()
                .filter_map(|event| match event {
                    AgentTurnEvent::RuntimeInfo(info) => Some(native_thinking(info)),
                    _ => None,
                })
                .collect();
            if valid.is_some() || command == "thinking-handled" {
                assert_eq!(levels, [expected], "{name}: {command}");
            } else {
                assert!(levels.is_empty(), "{name}: {command}");
            }
            session.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn queued_thinking_notifications_cannot_override_state_snapshots() {
    for scenario in [
        "stale-thinking-event",
        "stale-thinking-open",
        "newer-thinking-event",
        "stale-thinking-admission",
    ] {
        let (directory, client, mut spec) = fixture(scenario);
        let mut session = client.create_session(&spec).await.unwrap();
        if scenario == "stale-thinking-open" {
            assert!(session.poll_turn().unwrap().is_none());
            assert_eq!(
                native_thinking(&session.runtime_info().await.unwrap()),
                Some("low")
            );
        } else if scenario != "stale-thinking-admission" {
            spec.config.model = Some("fixture/other".into());
        }
        let command = if scenario == "stale-thinking-admission" {
            "thinking-handled"
        } else {
            "hello"
        };
        session.start_turn(command, &spec.config).await.unwrap();
        let events = terminal(session.as_mut()).await;
        assert_eq!(
            native_thinking(&session.runtime_info().await.unwrap()),
            Some("low")
        );
        assert!(!events.iter().any(|event| matches!(event,
            AgentTurnEvent::RuntimeInfo(info) if native_thinking(info) == Some("high"))));
        session.close().await.unwrap();
        let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
        for command in requests
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
        {
            if command["type"] == "set_thinking_level" {
                assert_eq!(command["level"], "low");
            }
        }
    }
}

#[tokio::test]
async fn conflicting_thinking_snapshots_bound_reads_without_changing_native_preferences() {
    let (directory, client, mut spec) = fixture("unstable-thinking-events");
    let mut session = client.create_session(&spec).await.unwrap();
    spec.config.model = Some("fixture/other".into());

    assert_eq!(
        session.start_turn("hello", &spec.config).await,
        Err(AgentSessionError::Rejected)
    );
    let diagnostic = session.failure_diagnostic().unwrap();
    assert_eq!(diagnostic.source, "thinking_conflict");
    assert_eq!(diagnostic.category, Category::Protocol);
    assert!(session.failure_message().unwrap().contains("kept changing"));
    session.close().await.unwrap();

    let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    let commands: Vec<Value> = requests
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap()["type"].clone())
        .collect();
    assert_eq!(commands, vec![json!("get_state"); 4]);
}

#[tokio::test]
async fn model_metadata_changes_do_not_publish_identical_runtime_info() {
    let (_directory, client, spec) = fixture("model-metadata-change");
    let mut session = client.create_session(&spec).await.unwrap();
    let initial = session.runtime_info().await.unwrap();
    for _ in 0..2 {
        session.start_turn("hello", &spec.config).await.unwrap();
        let events = terminal(session.as_mut()).await;
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, AgentTurnEvent::RuntimeInfo(_)))
        );
        assert_eq!(session.runtime_info().await.unwrap(), initial);
    }
    session.close().await.unwrap();
}

#[test]
fn catalog_uses_known_map_keys_when_seeding_defaults() {
    for (mapping, level, expected_options, expected_default) in [
        (
            json!({"future-level":"native","future":null}),
            "future-level",
            vec!["off", "minimal", "low", "medium", "high"],
            Some("off"),
        ),
        (
            json!({"off":null,"future-level":"native"}),
            "future-level",
            vec!["minimal", "low", "medium", "high"],
            Some("minimal"),
        ),
        (
            json!({"off":null,"minimal":null,"low":null,"medium":null,"high":null,
                "xhigh":null,"max":null,"future-level":"native"}),
            "future-level",
            vec![],
            None,
        ),
        (
            json!({"ultra":"native","future-level":"native"}),
            "ultra",
            vec!["off", "minimal", "low", "medium", "high"],
            Some("off"),
        ),
    ] {
        let models = config::models(
            &json!({"models":[{"provider":"backend","id":"reasoner","reasoning":true,
                "thinkingLevelMap":mapping}]}),
            &json!({"model":{"provider":"backend","id":"reasoner"},"thinkingLevel":level}),
        )
        .unwrap();
        let options = models[0]["thinkingOptions"].as_array().unwrap();

        assert_eq!(
            options
                .iter()
                .map(|option| option["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            expected_options
        );
        assert_eq!(
            models[0]["defaultThinkingOptionId"].as_str(),
            expected_default
        );
        assert_eq!(
            options
                .iter()
                .filter(|option| option["isDefault"] == true)
                .map(|option| option["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            expected_default.into_iter().collect::<Vec<_>>()
        );
    }
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
        assert_eq!(created.config.as_ref(), Some(&spec.config));
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
            Some(level)
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
    for (scenario, selection, expected, control) in [
        ("mapped-thinking", None, "high", None),
        ("", Some("off"), "off", Some("off")),
        ("", Some("max"), "high", Some("high")),
        ("", Some("ultra"), "off", Some("off")),
        ("clamp-thinking", Some("medium"), "high", Some("medium")),
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
        if let Some(level) = control {
            assert_eq!(selections[0]["level"], level);
        }
    }
}

#[tokio::test]
async fn opening_preserves_native_thinking_across_model_selection() {
    let (_directory, client, mut spec) = fixture("low-thinking");
    spec.config.model = Some("fixture/model".into());

    let mut session = client.create_session(&spec).await.unwrap();

    assert_eq!(
        native_thinking(&session.runtime_info().await.unwrap()),
        Some("low")
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn resume_preserves_recorded_thinking_unless_explicitly_overridden() {
    use std::io::Write;

    for selection in [None, Some("high")] {
        let (_directory, client, mut spec) = fixture("");
        spec.config.model = Some("fixture/model".into());
        spec.config.thinking_option_id = Some("high".into());
        let mut session = client.create_session(&spec).await.unwrap();
        session.start_turn("hello", &spec.config).await.unwrap();
        terminal(session.as_mut()).await;
        let handle = session.persistence().unwrap();
        session.close().await.unwrap();
        let path = handle.native_handle.as_ref().unwrap().as_str().unwrap();
        let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        writeln!(
            file,
            "\n{}",
            json!({"type":"thinking_level_change","thinkingLevel":"low"})
        )
        .unwrap();
        spec.config.thinking_option_id = selection.map(str::to_owned);

        let mut resumed = client
            .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
            .await
            .unwrap();

        assert_eq!(
            native_thinking(&resumed.runtime_info().await.unwrap()),
            Some(selection.unwrap_or("low"))
        );
        resumed.close().await.unwrap();
    }
}

#[tokio::test]
async fn conflicting_thinking_during_opening_only_rejects_prompt_admission() {
    for (model, thinking) in [
        (None, None),
        (Some("fixture/model"), None),
        (Some("fixture/model"), Some("high")),
    ] {
        let (directory, client, mut spec) = fixture("unstable-thinking-open");
        spec.config.model = model.map(str::to_owned);
        spec.config.thinking_option_id = thinking.map(str::to_owned);

        let mut session = client.create_session(&spec).await.unwrap();

        assert_eq!(
            native_thinking(&session.runtime_info().await.unwrap()),
            Some("low")
        );
        assert_eq!(
            session.start_turn("hello", &spec.config).await,
            Err(AgentSessionError::Rejected)
        );
        session.close().await.unwrap();
        let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
        assert!(
            requests
                .lines()
                .all(|line| serde_json::from_str::<Value>(line).unwrap()["type"] != "prompt")
        );
    }
}

#[tokio::test]
async fn conflicting_thinking_does_not_fail_an_already_accepted_prompt() {
    let (_directory, client, spec) = fixture("unstable-thinking-handled");
    let mut session = client.create_session(&spec).await.unwrap();

    session
        .start_turn("thinking-handled", &spec.config)
        .await
        .unwrap();

    assert!(matches!(
        terminal(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    assert_eq!(
        native_thinking(&session.runtime_info().await.unwrap()),
        Some("low")
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn thinking_changes_apply_between_turns_and_clear_to_medium() {
    let (_directory, client, mut spec) = fixture("");
    let mut session = client.create_session(&spec).await.unwrap();
    for (selection, expected) in [
        (Some("high"), "high"),
        (Some("low"), "low"),
        (None, "medium"),
    ] {
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
async fn unrelated_config_changes_preserve_extension_thinking_until_model_or_selection_changes() {
    let (directory, client, mut spec) = fixture("multiple-models");
    spec.config.model = Some("fixture/model".into());
    spec.config.thinking_option_id = Some("high".into());
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("thinking-after", &spec.config)
        .await
        .unwrap();
    terminal(session.as_mut()).await;

    for (model, mode, expected) in [
        ("fixture/model", Some("default"), "low"),
        ("fixture/other", Some("default"), "high"),
        ("fixture/model", Some("default"), "high"),
    ] {
        spec.config.model = Some(model.into());
        spec.config.mode_id = mode.map(str::to_owned);
        session.start_turn("hello", &spec.config).await.unwrap();
        terminal(session.as_mut()).await;
        let info = session.runtime_info().await.unwrap();
        assert_eq!(info.model.as_deref(), Some(model));
        assert_eq!(native_thinking(&info), Some(expected));
    }
    for (selection, expected) in [
        (Some("minimal"), "minimal"),
        (Some("high"), "high"),
        (None, "medium"),
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
    assert_eq!(
        levels,
        ["high", "high", "high", "minimal", "high", "medium"]
    );
    let models: Vec<_> = controls
        .iter()
        .filter(|command| command["type"] == "set_model")
        .map(|command| command["modelId"].as_str().unwrap())
        .collect();
    assert_eq!(models, ["model", "other", "model"]);
    assert_eq!(controls[0]["type"], "set_model");
    assert_eq!(controls[1]["type"], "set_thinking_level");
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
                assert!(info.extra.is_none());
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
async fn non_object_state_replies_fail_cleanly_during_opening_and_admission() {
    use crate::service::agent_manager::{AgentManager, AgentManagerError, AgentRegistration};
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

    for reply in [
        json!("bad state"),
        json!(42),
        json!(true),
        json!([]),
        json!([{}]),
        Value::Null,
    ] {
        let (directory, client, spec) = fixture("");
        let reply_path = directory.path().join("state-reply.json");
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
        std::fs::write(&reply_path, reply.to_string()).unwrap();

        assert_eq!(
            manager.send("agent", "hello").await,
            Err(AgentManagerError::Session)
        );
        assert!(manager.live_snapshot("agent").is_none());
        assert!(matches!(
            client.create_session(&spec).await,
            Err(AgentSessionError::Failed)
        ));

        let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
        assert!(
            requests.lines().all(|line| {
                serde_json::from_str::<Value>(line).unwrap()["type"] == "get_state"
            })
        );
        manager.close_all().await.unwrap();
    }
}

#[tokio::test]
async fn optional_thinking_snapshots_do_not_conflict_with_repeated_notifications() {
    for snapshot in [
        json!({}),
        json!({"thinkingLevel":null}),
        json!({"thinkingLevel":42}),
        json!({"thinkingLevel":""}),
        json!({"thinkingLevel":"bad\nlevel"}),
        json!({"thinkingLevel":"a".repeat(513)}),
    ] {
        let (directory, client, spec) = fixture("notified-optional-thinking-state");
        std::fs::write(
            directory.path().join("thinking-state.json"),
            snapshot.to_string(),
        )
        .unwrap();
        let mut session = client.create_session(&spec).await.unwrap();

        for _ in 0..2 {
            session.start_turn("hello", &spec.config).await.unwrap();
            assert!(matches!(
                terminal(session.as_mut()).await.last(),
                Some(AgentTurnEvent::Completed(_))
            ));
            assert_eq!(
                native_thinking(&session.runtime_info().await.unwrap()),
                None
            );
            assert!(session.failure_diagnostic().is_none());
        }

        session.close().await.unwrap();
        let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
        assert_eq!(
            requests
                .lines()
                .filter(|line| {
                    serde_json::from_str::<Value>(line).unwrap()["type"] == "get_state"
                })
                .count(),
            3
        );
    }
}

#[tokio::test]
async fn malformed_compaction_state_fails_admission_while_idle_or_streaming() {
    for streaming in [false, true] {
        for compacting in [Value::Null, json!(42), json!("false")] {
            let (directory, client, spec) = fixture("");
            let mut session = client.create_session(&spec).await.unwrap();
            std::fs::write(
                directory.path().join("state-overrides.json"),
                json!({"isStreaming":streaming,"isCompacting":compacting}).to_string(),
            )
            .unwrap();

            assert_eq!(
                session.start_turn("hello", &spec.config).await,
                Err(AgentSessionError::Failed)
            );

            session.close().await.unwrap();
            let requests =
                std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
            assert!(requests.lines().all(|line| {
                serde_json::from_str::<Value>(line).unwrap()["type"] == "get_state"
            }));
        }
    }
}

#[tokio::test]
async fn missing_thinking_state_clears_previous_runtime_level() {
    let (_directory, client, spec) = fixture("thinking-state-disappears");
    let mut session = client.create_session(&spec).await.unwrap();
    assert_eq!(
        native_thinking(&session.runtime_info().await.unwrap()),
        Some("medium")
    );
    session.start_turn("hello", &spec.config).await.unwrap();
    let events = terminal(session.as_mut()).await;
    assert!(events.iter().any(|event| matches!(event,
        AgentTurnEvent::RuntimeInfo(info) if info.thinking_option_id.is_none())));
    assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
    assert!(
        session
            .runtime_info()
            .await
            .unwrap()
            .thinking_option_id
            .is_none()
    );
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
        ("idle-thinking-missing", "medium"),
        ("idle-thinking-malformed", "medium"),
    ] {
        let (_directory, client, spec) = fixture(scenario);
        let mut session = client.create_session(&spec).await.unwrap();
        // Wait for the event emitted after the initial snapshot to reach the reader.
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Some(event) = session.poll_turn().unwrap() {
                    assert!(matches!(event, AgentTurnEvent::RuntimeInfo(_)), "{event:?}");
                }
                if native_thinking(&session.runtime_info().await.unwrap()) == Some(expected) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
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
    let mut session = client.create_session(&spec).await.unwrap();
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
            .contains("Pi rejected the thinking level")
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn pending_thinking_preferences_preserve_the_last_native_runtime() {
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
            assert_eq!(effective_thinking_option_id(&record), Some("high"));
            assert_eq!(
                native_thinking(record.runtime_info.as_ref().unwrap()),
                Some("high")
            );
        }
    }
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn runtime_readback_uses_native_identity_without_rewriting_preferences() {
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
            Some(if selection.is_some() {
                "high"
            } else {
                "medium"
            })
        );
        manager.send("agent", "thinking-after").await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                manager.poll().await.unwrap();
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
async fn model_only_updates_preserve_effective_thinking() {
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
            Some(selection.unwrap_or("medium"))
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
            Some("low")
        );
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn a_single_notification_remains_ambiguous_until_a_snapshot_catches_up() {
    let (directory, client, mut spec) = fixture("single-thinking-event");
    let mut session = client.create_session(&spec).await.unwrap();
    spec.config.model = Some("fixture/model".into());
    assert_eq!(
        session.start_turn("keep this", &spec.config).await,
        Err(AgentSessionError::Rejected)
    );
    assert_eq!(
        session.failure_diagnostic().unwrap().source,
        "thinking_conflict"
    );
    let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    let commands: Vec<Value> = requests
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap()["type"].clone())
        .collect();
    assert_eq!(commands, vec![json!("get_state"); 4]);
    std::fs::write(directory.path().join("settle-thinking"), "").unwrap();
    session.start_turn("keep this", &spec.config).await.unwrap();
    terminal(session.as_mut()).await;
    assert_eq!(
        native_thinking(&session.runtime_info().await.unwrap()),
        Some("high")
    );
    session.close().await.unwrap();
}
