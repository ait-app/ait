//! Regression probes for compatibility gaps found during the default-ACP audit.

use provider::storage::timeline::Timeline;

use super::*;

#[tokio::test]
async fn accepted_large_input_keeps_complete_durable_history() {
    let (directory, client, spec) = fixture("large-input");
    let mut session = open(&client, &spec, None).await.unwrap();
    let prompt = AgentPrompt {
        text: format!("input:{}", "\u{0001}".repeat(180 * 1024)),
        client_message_id: Some("large-input".to_owned()),
        ..Default::default()
    };
    prompt.validate().unwrap();
    session.start_input(&prompt, &spec.config).await.unwrap();
    let events = finish(&mut session).await;
    let timeline = Timeline::open(&directory.path().join("timeline.sqlite3")).unwrap();
    let inputs: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Timeline(entry) if entry.item["type"] == "user_message" => {
                Some(entry.clone())
            }
            _ => None,
        })
        .collect();
    provider::testing::append_timeline(&timeline, "agent", PROVIDER, &inputs).unwrap();
    let recovered: String = inputs
        .iter()
        .map(|entry| entry.item["text"].as_str().unwrap())
        .collect();
    assert_eq!(recovered, prompt.text);
    assert_eq!(
        inputs
            .iter()
            .filter(|entry| entry.item["clientMessageId"] == "large-input")
            .count(),
        1
    );
    assert_eq!(
        events.last(),
        Some(&AgentTurnEvent::Completed(Some("accepted".to_owned())))
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn later_config_acknowledgements_cannot_silently_reset_model_or_effort() {
    for scenario in ["drift-model", "drift-effort"] {
        for model in [None, Some("gemini-test".to_owned())] {
            let (directory, client, mut spec) = fixture(scenario);
            spec.config.model = model;
            spec.config.thinking_option_id = Some("low".to_owned());
            spec.config.mode_id = Some("plan".to_owned());
            let result = open(&client, &spec, None).await;
            assert!(
                result.is_err(),
                "{scenario} accepted a different effective configuration"
            );
            assert!(
                logs(&directory)
                    .iter()
                    .all(|frame| frame["rpc"]["method"] != "session/prompt")
            );
        }
    }
}

#[test]
fn recorded_shell_output_and_exit_status_survive_projection() {
    let recording: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/acp-shell.json")).unwrap();
    let mut projection =
        projection::Projection::new("turn", provider_native::images::ImageStore::default());
    for update in recording["updates"].as_array().unwrap() {
        projection.update(update).unwrap();
    }
    let completed = projection
        .stream
        .events
        .iter()
        .find_map(|event| match event {
            AgentTurnEvent::Timeline(entry) if entry.item["status"] == "completed" => Some(entry),
            _ => None,
        })
        .unwrap();
    assert_eq!(completed.item["detail"]["type"], "shell");
    assert!(
        completed.item["detail"]["output"]
            .as_str()
            .is_some_and(|output| output.contains("ACP_SHELL_OK"))
    );
    assert_eq!(completed.item["detail"]["exitCode"], 0);
}

#[tokio::test]
async fn execution_worker_restores_acp_history_images_and_usage_without_replaying_input() {
    use domain::agent_runtime::registry::AgentRuntimeRegistry;
    use provider::testing::{request, worker_at};

    let (directory, client, spec) = fixture("");
    let client = client.with_image_directory(directory.path().join("images"));
    let (worker, registry) = worker_at(
        directory.path(),
        Path::new(&spec.cwd),
        model::creation::Creations::default(),
        Box::new(client.clone()),
    );
    let created = request(&worker, "agent.create.request", json!({
        "config":{"provider":PROVIDER,"cwd":spec.cwd,"title":"ACP audit"},
        "initialPrompt":"first", "clientMessageId":"image-input",
        "images":[{"mimeType":"image/png","data":"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aKd0AAAAASUVORK5CYII="}]
    })).await;
    let id = created["agentId"].as_str().unwrap();
    assert_eq!(
        request(&worker, "agent.finish.wait.request", json!({"agentId":id})).await["status"],
        "idle"
    );
    let saved = registry.get(id).unwrap().unwrap().persistence.unwrap();
    assert_eq!(saved.metadata.as_ref().unwrap()["transport"], "acp");
    assert_eq!(
        saved.metadata.as_ref().unwrap()["acpUsage"]["inputTokens"],
        10
    );
    worker.shutdown().await.unwrap();

    let (worker, registry) = worker_at(
        directory.path(),
        Path::new(&spec.cwd),
        model::creation::Creations::default(),
        Box::new(client),
    );
    let resumed = request(&worker, "agent.resume.request", json!({"handle":saved})).await;
    assert_eq!(resumed["agentId"], id);
    for text in ["second", "third"] {
        assert_eq!(
            request(
                &worker,
                "agent.message.send.request",
                json!({"agentId":id,"text":text})
            )
            .await["accepted"],
            true
        );
        let result = request(&worker, "agent.finish.wait.request", json!({"agentId":id})).await;
        assert_eq!(result["status"], "idle");
        assert_eq!(result["lastMessage"], text);
    }
    let saved_after = registry.get(id).unwrap().unwrap().persistence.unwrap();
    assert_eq!(saved_after.session_id, saved.session_id);
    assert_eq!(
        saved_after.metadata.as_ref().unwrap()["acpUsage"]["inputTokens"],
        30
    );
    worker.shutdown().await.unwrap();
    let timeline = Timeline::open(&directory.path().join("timeline.sqlite")).unwrap();
    let (_, rows) = provider::testing::timeline_entries(&timeline, id).unwrap();
    let inputs: Vec<_> = rows
        .iter()
        .filter(|entry| entry.item["type"] == "user_message")
        .collect();
    assert_eq!(
        inputs
            .iter()
            .map(|entry| entry.item["text"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["first", "second", "third"]
    );
    assert_eq!(inputs[0].item["clientMessageId"], "image-input");
    let image = inputs[0].item["imageSources"][0].as_str().unwrap();
    assert!(Path::new(image).is_file());
    let projected = provider::testing::projected_timeline_items(&timeline, id).unwrap();
    assert_eq!(
        projected
            .iter()
            .filter(|item| item["type"] == "assistant_message")
            .map(|item| item["text"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["first", "second", "third"]
    );
}

#[tokio::test]
async fn native_completion_with_unfinished_tools_is_not_reported_as_success() {
    let (_directory, client, spec) = fixture("");
    let mut session = open(&client, &spec, None).await.unwrap();
    session
        .start_turn("unfinished", &spec.config)
        .await
        .unwrap();
    let events = finish(&mut session).await;
    assert_eq!(events.last(), Some(&AgentTurnEvent::Failed));
    assert!(events.iter().any(|event| matches!(event, AgentTurnEvent::Timeline(item) if item.item["type"] == "tool_call" && item.item["status"] == "failed")));
    assert_eq!(
        events.iter().filter(|event| turn::terminal(event)).count(),
        1
    );
    assert!(session.poll_turn().unwrap().is_none());
    assert_eq!(
        session.failure_diagnostic().unwrap().category,
        Category::Protocol
    );
    session.close().await.unwrap();
}
