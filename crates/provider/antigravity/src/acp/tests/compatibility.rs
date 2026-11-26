//! Feature-local upgrade guidance, with no version-wide admission gate.

use super::*;

#[tokio::test]
async fn no_acp_command_has_upgrade_guidance_without_claiming_availability() {
    let (_directory, client, _spec) = fixture("no-acp");
    assert!(!client.is_available().await.unwrap());
    let diagnostic = client.diagnostic().await.unwrap();
    assert!(diagnostic.contains("ACP") && diagnostic.contains("1.5.15"));
}

#[tokio::test]
async fn missing_image_support_rejects_only_images_before_native_admission() {
    let (directory, client, spec) = fixture("missing-capability");
    let mut session = open(&client, &spec, None).await.unwrap();
    let prompt = AgentPrompt {
        text: "image".to_owned(),
        images: vec![provider_core::protocol::prompt::PromptImage {
            mime_type: "image/png".to_owned(),
            data: "aGVsbG8=".to_owned(),
        }],
        ..Default::default()
    };
    assert_eq!(
        session.start_input(&prompt, &spec.config).await,
        Err(AgentSessionError::Rejected)
    );
    let explanation = session.failure_message().unwrap();
    assert!(explanation.contains("image") && explanation.contains("1.5.15"));
    assert!(
        logs(&directory)
            .iter()
            .all(|frame| frame["rpc"]["method"] != "session/prompt")
    );
    session
        .start_turn("text remains usable", &spec.config)
        .await
        .unwrap();
    assert!(matches!(
        finish(&mut session).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn missing_resume_keeps_current_chat_and_rejects_config_switch_before_closing() {
    let (directory, client, spec) = fixture("missing-resume");
    let mut session = open(&client, &spec, None).await.unwrap();
    session.start_turn("first", &spec.config).await.unwrap();
    let events = finish(&mut session).await;
    assert!(events.iter().any(|event| matches!(event, AgentTurnEvent::Timeline(item) if item.item["type"] == "notification" && item.item["message"].as_str().unwrap().contains("1.5.15"))));
    let changed = StoredAgentConfig {
        mode_id: Some("plan".to_owned()),
        ..spec.config.clone()
    };
    assert_eq!(
        session.start_turn("not submitted", &changed).await,
        Err(AgentSessionError::Rejected)
    );
    assert!(session.failure_message().unwrap().contains("1.5.15"));
    assert!(session.transport.is_some());
    assert_eq!(session.spec.config, spec.config);
    session
        .start_turn("continue original", &spec.config)
        .await
        .unwrap();
    assert!(matches!(
        finish(&mut session).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    let handle = session.handle();
    session.close().await.unwrap();
    assert!(matches!(
        open(&client, &spec, Some(&handle)).await,
        Err(OpenError::Unsupported)
    ));
    assert_eq!(
        logs(&directory)
            .iter()
            .filter(|frame| frame["rpc"]["method"] == "session/new")
            .count(),
        1
    );
    assert_eq!(
        logs(&directory)
            .iter()
            .filter(|frame| frame["rpc"]["method"] == "session/prompt")
            .count(),
        2
    );
}

#[test]
fn background_upgrade_notice_is_local_to_affected_tools_and_emitted_once() {
    for fixed in [false, true] {
        let mut projection =
            projection::Projection::new("turn", provider_native::images::ImageStore::default());
        projection.completion_fixes = fixed;
        projection.update(&json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hello"}})).unwrap();
        assert!(projection.stream.events.iter().all(|event| !matches!(event, AgentTurnEvent::Timeline(item) if item.item["type"] == "notification")));
        for (id, name) in [("child", "invoke_subagent"), ("shell", "run_command")] {
            projection.update(&json!({"sessionUpdate":"tool_call","toolCallId":id,"name":name,"status":"completed","rawInput":{}})).unwrap();
        }
        let notices = projection.stream.events.iter().filter(|event| matches!(event, AgentTurnEvent::Timeline(item) if item.item["type"] == "notification" && item.item["message"].as_str().unwrap().contains("1.5.15"))).count();
        assert_eq!(notices, usize::from(!fixed));
    }
}

#[tokio::test]
async fn old_cancellation_is_still_available_and_explains_native_limitations() {
    let (_directory, mut client, spec) = fixture("");
    recovery::set_version(&mut client, "1.5.13");
    let mut session = open(&client, &spec, None).await.unwrap();
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    loop {
        if matches!(next(&mut session).await, AgentTurnEvent::Progress { entry, .. } if entry.item["type"] == "tool_call")
        {
            break;
        }
    }
    session.cancel_turn(&turn).await.unwrap();
    let events = finish(&mut session).await;
    assert_eq!(events.last(), Some(&AgentTurnEvent::Cancelled));
    assert_eq!(
        events.iter().filter(|event| turn::terminal(event)).count(),
        1
    );
    assert!(events.iter().any(|event| matches!(event, AgentTurnEvent::Timeline(item) if item.item["type"] == "notification" && item.item["message"].as_str().unwrap().contains("1.5.15"))));
    session.close().await.unwrap();
}

#[tokio::test]
async fn completed_usage_preserves_context_and_does_not_add_it_to_consumption() {
    let (_directory, client, spec) = fixture("");
    let mut session = open(&client, &spec, None).await.unwrap();
    for input in [10, 20] {
        session.start_turn("usage", &spec.config).await.unwrap();
        let events = finish(&mut session).await;
        let last = events
            .iter()
            .rev()
            .find_map(|event| match event {
                AgentTurnEvent::Usage(usage) => Some(usage),
                _ => None,
            })
            .unwrap();
        assert_eq!(last.input_tokens, Some(input));
        assert_eq!(last.context_window_used_tokens, Some(42));
        assert_eq!(last.context_window_max_tokens, Some(1000));
        assert!(
            session.handle().metadata.unwrap()["acpUsage"]
                .get("contextWindowUsedTokens")
                .is_none()
        );
    }
    session.close().await.unwrap();
}

#[test]
fn native_recorded_write_content_remains_visible_in_the_tool_card() {
    let recording = include_str!("../../tests/fixtures/acp-native.ndjson");
    let mut projection =
        projection::Projection::new("turn", provider_native::images::ImageStore::default());
    for line in recording.lines() {
        let frame: Value = serde_json::from_str(line).unwrap();
        if frame["type"] == "stdout"
            && let Some(update) = frame.pointer("/frame/params/update")
        {
            projection.update(update).unwrap();
        }
    }
    let detail = projection
        .stream
        .events
        .iter()
        .find_map(|event| match event {
            AgentTurnEvent::Timeline(item) if item.item["detail"]["type"] == "write" => {
                Some(&item.item["detail"])
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(detail["content"], "ACP_FILE_OK");
}

#[tokio::test]
async fn upgrade_notices_and_complete_usage_survive_runtime_storage() {
    use std::os::unix::fs::PermissionsExt;

    use domain::agent_runtime::registry::AgentRuntimeRegistry;
    use provider::storage::timeline::Timeline;
    use provider::testing::{request, worker_at};

    let (directory, mut client, spec) = fixture("missing-resume");
    // The worker intentionally replaces client environment overrides with its launch environment.
    // Keep the stdio peer's test scenario in an executable wrapper, not production environment logic.
    let program = directory.path().join("agy-fixture");
    let fixture_program = serde_json::to_string(&client.program).unwrap();
    let log = serde_json::to_string(&directory.path().join("requests.jsonl")).unwrap();
    std::fs::write(&program, format!(
        "#!/usr/bin/env python3\nimport os,sys\nos.environ['AGY_ACP_SCENARIO']='missing-resume'\nos.environ['AGY_FIXTURE_LOG']={log}\nos.execv({fixture_program}, [{fixture_program}, *sys.argv[1:]])\n"
    )).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    client.program = program;
    let (worker, registry) = worker_at(
        directory.path(),
        Path::new(&spec.cwd),
        model::creation::Creations::default(),
        Box::new(client),
    );
    let created = request(
        &worker,
        "agent.create.request",
        json!({
            "config":{"provider":PROVIDER,"cwd":spec.cwd,"title":"compatibility"},
            "initialPrompt":"ordinary text"
        }),
    )
    .await;
    let id = created["agentId"].as_str().unwrap();
    assert_eq!(
        request(&worker, "agent.finish.wait.request", json!({"agentId":id})).await["status"],
        "idle"
    );
    let record = registry.get(id).unwrap().unwrap();
    let last_usage = &record.runtime_info.unwrap().extra.unwrap()["lastUsage"];
    assert_eq!(last_usage["inputTokens"], 10);
    assert_eq!(last_usage["contextWindowUsedTokens"], 42);
    assert_eq!(last_usage["contextWindowMaxTokens"], 1000);
    worker.shutdown().await.unwrap();
    let timeline = Timeline::open(&directory.path().join("timeline.sqlite")).unwrap();
    let projected = provider::testing::projected_timeline_items(&timeline, id).unwrap();
    assert!(projected.iter().any(|item| item["type"] == "notification"
        && item["message"].as_str().unwrap().contains("1.5.15")));
    let frames = logs(&directory);
    let prompts: Vec<_> = frames
        .iter()
        .filter(|frame| frame["rpc"]["method"] == "session/prompt")
        .collect();
    assert_eq!(prompts.len(), 1);
    assert_eq!(
        prompts[0]["rpc"]["params"]["prompt"],
        json!([{"type":"text","text":"ordinary text"}])
    );
}
