use super::*;
use crate::ports::agent_session::{AgentClient, AgentResumePurpose};
use crate::ports::environment::AgentEnvironment;

fn fixture() -> (tempfile::TempDir, OmpClient, AgentSessionSpec) {
    let root = tempfile::tempdir().unwrap();
    let program = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/local/omp/acp/tests/fixtures/omp.py");
    let client = OmpClient {
        environment: AgentEnvironment::try_from(BTreeMap::from([(
            "PI_CODING_AGENT_DIR".to_owned(),
            root.path().join("agent").to_str().unwrap().to_owned(),
        )]))
        .unwrap(),
        ..OmpClient::new(program).with_image_directory(root.path().join("images"))
    };
    let spec = AgentSessionSpec {
        provider: "omp".to_owned(),
        cwd: root.path().to_str().unwrap().to_owned(),
        config: StoredAgentConfig::default(),
    };
    (root, client, spec)
}

async fn next(session: &mut dyn AgentSession) -> Result<AgentTurnEvent, AgentSessionError> {
    super::super::tests::next(session).await
}

async fn complete(session: &mut dyn AgentSession) -> Vec<AgentTurnEvent> {
    super::super::tests::complete(session).await
}

#[tokio::test]
async fn native_history_preserves_acp_selection_and_receipts_when_resumed() {
    let (_root, client, mut spec) = fixture();
    spec.config.model = Some(json!(["other", "model"]).to_string());
    spec.config.thinking_option_id = Some("high".to_owned());
    spec.config.mode_id = Some("plan".to_owned());
    let mut session = client.create_session(&spec).await.unwrap();
    let info = session.runtime_info().await.unwrap();
    assert_eq!(info.model, spec.config.model);
    assert_eq!(info.thinking_option_id, spec.config.thinking_option_id);
    assert_eq!(info.mode_id, spec.config.mode_id);
    let prompt = AgentPrompt {
        client_message_id: Some("receipt".to_owned()),
        ..AgentPrompt::text("hello")
    };
    session.start_input(&prompt, &spec.config).await.unwrap();
    assert_eq!(
        session.start_turn("duplicate", &spec.config).await,
        Err(AgentSessionError::Rejected)
    );
    let events = complete(session.as_mut()).await;
    assert!(
        matches!(events.last(),Some(AgentTurnEvent::Completed(Some(text))) if text=="Hello ACP")
    );
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let history = client.inspect_session(&handle, &spec.cwd).await.unwrap();
    assert_eq!(history.config.feature_values.unwrap()["transport"], "acp");
    assert_eq!(history.entries[0].item["clientMessageId"], "receipt");
    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    resumed.start_turn("again", &spec.config).await.unwrap();
    complete(resumed.as_mut()).await;
    assert_eq!(resumed.persistence().unwrap().session_id, handle.session_id);
    resumed.close().await.unwrap();
    assert_eq!(
        client
            .history(&resumed.persistence().unwrap(), &spec.cwd)
            .await
            .unwrap()[0]
            .item["clientMessageId"],
        "receipt"
    );
}

#[tokio::test]
async fn prompt_response_and_previous_settlement_cannot_drop_background_continuation() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    for _ in 0..2 {
        let turn = session
            .start_turn("background", &spec.config)
            .await
            .unwrap();
        loop {
            if matches!(next(session.as_mut()).await.unwrap(),AgentTurnEvent::Progress {entry,..} if entry.item["text"]=="Waiting for background result")
            {
                break;
            }
        }
        assert!(session.start_turn("too early", &spec.config).await.is_err());
        for _ in 0..8 {
            assert!(!matches!(
                session.poll_turn().unwrap(),
                Some(
                    AgentTurnEvent::Completed(_)
                        | AgentTurnEvent::Cancelled
                        | AgentTurnEvent::Failed
                )
            ));
        }
        std::fs::write(root.path().join("agent").join(format!("wake-{turn}")), "").unwrap();
        let events = complete(session.as_mut()).await;
        assert!(
            matches!(events.last(),Some(AgentTurnEvent::Completed(Some(text))) if text=="Background continued")
        );
        let history = events
            .iter()
            .find_map(|event| {
                if let AgentTurnEvent::History(entries) = event {
                    Some(entries)
                } else {
                    None
                }
            })
            .unwrap();
        assert!(
            history
                .iter()
                .any(|entry| entry.item["text"] == "Background continued")
        );
    }
    session.close().await.unwrap();
}

#[tokio::test]
async fn callbacks_survive_steering_responses_and_questions_keep_comma_choices() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    session.steer_turn(&turn, "race").await.unwrap();
    let request = loop {
        if let AgentTurnEvent::PermissionRequested(request) = next(session.as_mut()).await.unwrap()
        {
            break request;
        }
    };
    assert_eq!(request["provider"], "omp");
    session
        .respond_permission(
            request["id"].as_str().unwrap(),
            &json!({"behavior":"allow"}),
        )
        .await
        .unwrap();
    complete(session.as_mut()).await;
    session.start_turn("question", &spec.config).await.unwrap();
    let request = loop {
        if let AgentTurnEvent::PermissionRequested(request) = next(session.as_mut()).await.unwrap()
        {
            break request;
        }
    };
    let id = request["id"].as_str().unwrap();
    assert!(
        session
            .respond_permission(
                id,
                &json!({"behavior":"allow","updatedInput":{"content":{"values":["A, B","A, B"]}}})
            )
            .await
            .is_err()
    );
    session
        .respond_permission(
            id,
            &json!({"behavior":"allow","updatedInput":{"content":{"values":["A, B","C"]}}}),
        )
        .await
        .unwrap();
    complete(session.as_mut()).await;
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    session.cancel_turn(&turn).await.unwrap();
    assert!(matches!(
        complete(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Cancelled)
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn native_failure_details_survive_close_and_protocol_failures_never_replay_prompts() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("error", &spec.config).await.unwrap();
    let events = complete(session.as_mut()).await;
    assert!(matches!(events.last(), Some(AgentTurnEvent::Failed)));
    for event in events {
        let text = match event {
            AgentTurnEvent::Timeline(entry) | AgentTurnEvent::Progress { entry, .. } => {
                entry.item.to_string()
            }
            AgentTurnEvent::History(entries) => serde_json::to_string(&entries).unwrap(),
            _ => continue,
        };
        assert!(!text.contains("fixture-secret"));
    }
    let message = session.failure_message().unwrap().into_owned();
    assert!(message.contains("Upstream shard unavailable"));
    assert!(!message.contains("fixture-secret"));
    session.close().await.unwrap();
    assert_eq!(session.failure_message().unwrap(), message);
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("malformed", &spec.config).await.unwrap();
    while next(session.as_mut()).await.is_ok() {}
    let _ = session.close().await;
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert_eq!(
        calls
            .lines()
            .filter(|line| line.contains("\"method\": \"session/prompt\""))
            .count(),
        2
    );
}

#[tokio::test]
async fn draft_discovery_is_ephemeral_and_empty_handles_never_manufacture_input() {
    let (root, client, spec) = fixture();
    let features = client.draft_features(&spec).await.unwrap();
    assert!(features.iter().any(|feature| feature["id"] == "autoRetry"));
    assert!(!features.iter().any(|feature| feature["id"] == "transport"));
    assert_eq!(client.commands(&spec).await.unwrap()[0]["name"], "review");
    assert!(!root.path().join("agent/sessions").exists());
    let mut session = client.create_session(&spec).await.unwrap();
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    assert_eq!(client.history(&handle, &spec.cwd).await.unwrap().len(), 0);
    let mut session = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    session
        .start_turn("first real input", &spec.config)
        .await
        .unwrap();
    complete(session.as_mut()).await;
    session.close().await.unwrap();
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert_eq!(
        calls
            .lines()
            .filter(|line| line.contains("\"method\": \"session/prompt\""))
            .count(),
        1
    );
    std::fs::write(root.path().join("agent/old-acp"), "").unwrap();
    assert!(client.create_session(&spec).await.is_err());
}

#[tokio::test]
async fn manager_keeps_background_work_live_and_persists_native_failure_detail() {
    use crate::service::agent_manager::{AgentManager, AgentRegistration};
    use crate::storage::timeline::Timeline;
    use domain::agent_runtime::{AgentRuntimeStatus, registry::AgentRuntimeRegistry};
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

    let (root, client, spec) = fixture();
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    let timeline = Timeline::open(&root.path().join("timeline.sqlite")).unwrap();
    let mut manager = AgentManager::new(Box::new(registry.clone())).with_timeline(timeline.clone());
    manager.register_client(Box::new(client)).unwrap();
    manager
        .create("agent", &spec, AgentRegistration::default())
        .await
        .unwrap();
    manager.send("agent", "background").await.unwrap();
    let turn = manager.active_turn("agent").unwrap().to_owned();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            manager.poll().await.unwrap();
            if timeline
                .read("agent")
                .unwrap()
                .1
                .iter()
                .any(|row| row.entry.item["text"] == "Waiting for background result")
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(manager.active_turn("agent"), Some(turn.as_str()));
    std::fs::write(root.path().join("agent").join(format!("wake-{turn}")), "").unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while manager.active_turn("agent").is_some() {
            manager.poll().await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    let record = registry.get("agent").unwrap().unwrap();
    assert_eq!(
        record.last_status,
        AgentRuntimeStatus::Idle,
        "{:?}",
        record.last_error
    );
    assert!(manager.live_snapshot("agent").is_some());
    manager.send("agent", "error").await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while manager.active_turn("agent").is_some() {
            manager.poll().await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    let failure = registry.get("agent").unwrap().unwrap().last_error.unwrap();
    assert!(failure.contains("Upstream shard unavailable"));
    assert!(!failure.contains("fixture-secret"));
    let (_, rows) = timeline.read("agent").unwrap();
    assert!(rows.iter().any(|row| {
        row.entry.item["type"] == "error"
            && row.entry.item["message"]
                .as_str()
                .is_some_and(|text| text.contains("Upstream shard unavailable"))
    }));
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn default_and_legacy_settings_always_launch_acp_and_resume_native_identity() {
    let (root, client, mut spec) = fixture();
    for transport in [None, Some("rpc"), Some("acp")] {
        spec.config.feature_values = transport.map(|transport| {
            BTreeMap::from([
                ("transport".to_owned(), json!(transport)),
                ("autoRetry".to_owned(), json!(false)),
            ])
        });
        let mut session = client.create_session(&spec).await.unwrap();
        let prompt = AgentPrompt {
            client_message_id: Some("legacy-receipt".to_owned()),
            ..AgentPrompt::text("hello")
        };
        session.start_input(&prompt, &spec.config).await.unwrap();
        complete(session.as_mut()).await;
        let mut handle = session.persistence().unwrap();
        session.close().await.unwrap();
        handle.metadata.as_mut().unwrap().remove("transport");
        let mut resumed = client
            .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
            .await
            .unwrap();
        assert_eq!(resumed.persistence().unwrap().session_id, handle.session_id);
        resumed.start_turn("again", &spec.config).await.unwrap();
        complete(resumed.as_mut()).await;
        resumed.close().await.unwrap();
        let history = client.inspect_session(&handle, &spec.cwd).await.unwrap();
        assert_eq!(history.config.feature_values.unwrap()["transport"], "acp");
        assert_eq!(history.entries[0].item["clientMessageId"], "legacy-receipt");
    }
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    let launches: Vec<Value> = calls
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|call| call.get("args").is_some())
        .collect();
    assert!(
        launches
            .iter()
            .all(|call| call["args"][0] == "--mode" && call["args"][1] == "acp")
    );
}

#[tokio::test]
async fn native_controls_keep_overrides_and_probes_do_not_apply_them() {
    let (root, client, mut spec) = fixture();
    spec.config.feature_values = Some(BTreeMap::from([
        ("autoRetry".to_owned(), json!(false)),
        ("autoCompaction".to_owned(), json!(false)),
        ("fastMode".to_owned(), json!(true)),
        ("slowMode".to_owned(), json!(true)),
    ]));
    let features = client.draft_features(&spec).await.unwrap();
    assert_eq!(features.len(), 4);
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(!calls.contains("session/set_config_option"));
    assert!(!root.path().join("agent/sessions").exists());
    let mut session = client.create_session(&spec).await.unwrap();
    let controls = session.control_settings(&spec.config).unwrap();
    assert_eq!(controls["features"][0]["value"], false);
    assert!(
        controls["availableModes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|mode| mode["id"] == "full" && mode["isUnattended"] == true)
    );
    spec.config.mode_id = Some("ask-cached".to_owned());
    session.start_turn("hello", &spec.config).await.unwrap();
    complete(session.as_mut()).await;
    assert_eq!(
        session.runtime_info().await.unwrap().mode_id,
        spec.config.mode_id
    );
    session.close().await.unwrap();
    let calls: Vec<Value> = std::fs::read_to_string(root.path().join("agent/calls.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for (key, value) in spec.config.feature_values.as_ref().unwrap() {
        assert!(
            calls
                .iter()
                .any(|call| call["method"] == "session/set_config_option"
                    && call["params"]["configId"] == *key
                    && call["params"]["value"] == *value)
        );
    }
    spec.config.model = Some(json!(["other", "model"]).to_string());
    assert_eq!(client.draft_features(&spec).await.unwrap().len(), 2);
    assert!(client.create_session(&spec).await.is_err());
}

#[tokio::test]
async fn acp_discovery_and_live_tiers_follow_each_model_without_rpc() {
    let (_root, client, mut spec) = fixture();
    let details = client.discover(&spec.cwd).await.unwrap();
    assert!(details.models.iter().all(|model| {
        model["thinkingOptions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|option| option["id"] == "max")
    }));
    let mut session = client.create_session(&spec).await.unwrap();
    for (provider, expected) in [("other", 2), ("test", 4)] {
        spec.config.model = Some(json!([provider, "model"]).to_string());
        assert_eq!(
            session.control_settings(&spec.config).unwrap()["features"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        session.start_turn("hello", &spec.config).await.unwrap();
        complete(session.as_mut()).await;
        assert_eq!(
            session.control_settings(&spec.config).unwrap()["features"]
                .as_array()
                .unwrap()
                .len(),
            expected
        );
    }
    session.close().await.unwrap();
}

#[tokio::test]
async fn acp_preserves_native_usage_children_and_large_output() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("usage", &spec.config).await.unwrap();
    let events = complete(session.as_mut()).await;
    assert!(events.iter().any(|event| matches!(event, AgentTurnEvent::Usage(usage) if usage.input_tokens == Some(10) && usage.output_tokens == Some(4) && usage.cached_input_tokens == Some(2) && usage.total_cost_usd == Some(0.25) && usage.context_window_max_tokens == Some(64000))));
    session.start_turn("subagents", &spec.config).await.unwrap();
    let events = complete(session.as_mut()).await;
    assert!(events.iter().any(|event| matches!(event, AgentTurnEvent::Subagent(crate::ports::controls::SubagentEvent::Timeline { entry, .. }) if entry.item["text"] == "Child answer")));
    assert_eq!(session.subagents()[0].descriptor["status"], "completed");
    session.start_turn("large", &spec.config).await.unwrap();
    assert!(matches!(
        complete(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn system_prompt_changes_reopen_acp_without_changing_native_history() {
    let (root, client, mut spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("hello", &spec.config).await.unwrap();
    complete(session.as_mut()).await;
    let handle = session.persistence().unwrap();
    spec.config.system_prompt = Some("Updated instructions".to_owned());
    session.validate_config_update(&spec.config).unwrap();
    session.start_turn("again", &spec.config).await.unwrap();
    complete(session.as_mut()).await;
    assert_eq!(session.persistence().unwrap().session_id, handle.session_id);
    session.close().await.unwrap();
    let calls: Vec<Value> = std::fs::read_to_string(root.path().join("agent/calls.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(calls.iter().any(|call| {
        call["args"]
            .as_array()
            .is_some_and(|args| args.iter().any(|arg| arg == "Updated instructions"))
    }));
    assert_eq!(
        client
            .history(&handle, &spec.cwd)
            .await
            .unwrap()
            .iter()
            .filter(|entry| entry.item["type"] == "user_message")
            .count(),
        2
    );
}

#[tokio::test]
async fn missing_required_acp_features_fail_before_input_without_rpc_fallback() {
    let (root, client, spec) = fixture();
    std::fs::create_dir_all(root.path().join("agent")).unwrap();
    std::fs::write(root.path().join("agent/missing-features"), "").unwrap();
    assert!(client.create_session(&spec).await.is_err());
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(!calls.contains("session/prompt"));
    assert!(!calls.contains("rpc-ui"));
}

#[tokio::test]
async fn acp_rich_input_and_blob_history_preserve_images_and_reject_unsupported_schema() {
    use crate::protocol::prompt::PromptImage;
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let invalid = AgentPrompt {
        output_schema: Some(json!({"type":"object"})),
        ..AgentPrompt::text("invalid")
    };
    assert_eq!(
        session.start_input(&invalid, &spec.config).await,
        Err(AgentSessionError::Rejected)
    );
    let prompt = AgentPrompt {
        images: vec![PromptImage {
            data: "aW1hZ2U=".to_owned(),
            mime_type: "image/png".to_owned(),
        }],
        client_message_id: Some("image-receipt".to_owned()),
        ..AgentPrompt::text("describe")
    };
    session.start_input(&prompt, &spec.config).await.unwrap();
    let events = complete(session.as_mut()).await;
    assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let history = client.history(&handle, &spec.cwd).await.unwrap();
    assert!(
        history[0].item["text"]
            .as_str()
            .unwrap()
            .contains("![Image]")
    );
    assert_eq!(history[0].item["clientMessageId"], "image-receipt");
    let calls: Vec<Value> = std::fs::read_to_string(root.path().join("agent/calls.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let prompts: Vec<&Value> = calls
        .iter()
        .filter(|call| call["method"] == "session/prompt")
        .collect();
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0]["params"]["prompt"][1]["data"], "aW1hZ2U=");
    let initializes: Vec<&Value> = calls
        .iter()
        .filter(|call| call["method"] == "initialize")
        .collect();
    assert_eq!(
        initializes[0]["params"]["clientCapabilities"]["_meta"]["ompNativeMcp"],
        true
    );
}
