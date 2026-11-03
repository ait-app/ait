use super::*;

const CATALOG: &str = include_str!("../../../../tests/fixtures/omp_commands.json");

#[tokio::test]
async fn local_command_result_cannot_take_the_next_model_inputs_client_identity() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    for (text, id) in [
        ("/notify-only", "command-input"),
        ("next prompt", "model-input"),
    ] {
        let prompt = AgentPrompt {
            client_message_id: Some(id.to_owned()),
            ..AgentPrompt::text(text)
        };
        session.start_input(&prompt, &spec.config).await.unwrap();
        let events = complete(session.as_mut()).await;
        assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
        if id == "model-input" {
            assert!(
                events.iter().any(|event| matches!(event,
                AgentTurnEvent::Timeline(entry)
                    if entry.item["type"] == "user_message"
                        && entry.item["clientMessageId"] == id
                        && entry.item["text"] == text)),
                "{events:?}"
            );
        }
    }
    session.close().await.unwrap();
}

#[tokio::test]
async fn unsupported_draft_completion_reuses_discovered_capabilities_until_native_refresh() {
    let (root, client, spec) = fixture();
    client.commands(&spec).await.unwrap();
    let calls = root.path().join("agent/calls.jsonl");
    let before = std::fs::read_to_string(&calls).unwrap();
    let input = crate::protocol::commands::CompletionInput {
        text: "/advisor ".into(),
        cursor: 9,
    };
    for _ in 0..2 {
        assert!(
            !client
                .complete_commands(&spec, &input)
                .await
                .unwrap()
                .supported
        );
    }
    assert_eq!(std::fs::read_to_string(&calls).unwrap(), before);
    std::fs::write(
        root.path().join("agent/completion.json"),
        r#"{"supported":true,"items":[]}"#,
    )
    .unwrap();
    client.commands(&spec).await.unwrap();
    assert!(
        client
            .complete_commands(&spec, &input)
            .await
            .unwrap()
            .supported
    );
}

#[tokio::test]
async fn completion_is_negotiated_and_old_runtimes_receive_no_unknown_command() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let calls = root.path().join("agent/calls.jsonl");
    let before = std::fs::read_to_string(&calls).unwrap();
    let input = crate::protocol::commands::CompletionInput {
        text: "/advisor o".into(),
        cursor: 10,
    };
    assert!(!session.complete_commands(&input).await.unwrap().supported);
    assert_eq!(std::fs::read_to_string(&calls).unwrap(), before);
    session.close().await.unwrap();
}

#[tokio::test]
async fn native_completion_uses_the_existing_owner_and_rejects_invalid_edits() {
    let (root, client, spec) = fixture();
    let path = root.path().join("agent/completion.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let result = json!({"supported":true,"items":[{"id":"native-1","label":"third",
        "description":"Original third level", "insertText":"third ",
        "replaceRange":{"start":13,"end":14},"cursor":19,"triggerCompletion":true}]});
    std::fs::write(&path, result.to_string()).unwrap();
    let mut session = client.create_session(&spec).await.unwrap();
    let calls_path = root.path().join("agent/calls.jsonl");
    let owners_before = std::fs::read_to_string(&calls_path)
        .unwrap()
        .lines()
        .filter(|line| {
            serde_json::from_str::<Value>(line)
                .unwrap()
                .get("args")
                .is_some()
        })
        .count();
    let input = crate::protocol::commands::CompletionInput {
        text: "/nested next t suffix".into(),
        cursor: 14,
    };
    let response = session.complete_commands(&input).await.unwrap();
    assert!(response.supported);
    assert_eq!(response.items[0].insert_text, "third ");
    assert_eq!(response.items[0].cursor, Some(19));
    assert_eq!(
        response.items[0].description.as_deref(),
        Some("Original third level")
    );
    let log = std::fs::read_to_string(calls_path).unwrap();
    let calls: Vec<Value> = log
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.get("args").is_some())
            .count(),
        owners_before
    );
    assert!(calls.iter().any(|call| call["type"] == "complete"
        && call["text"] == input.text
        && call["cursor"] == 14));
    std::fs::write(
        &path,
        json!({"supported":true,"items":[{"id":"bad","label":"bad",
        "insertText":"x","replaceRange":{"start":90,"end":100}}]})
        .to_string(),
    )
    .unwrap();
    assert!(session.complete_commands(&input).await.is_err());
    session.close().await.unwrap();
}

#[tokio::test]
async fn live_command_metadata_tracks_native_updates_including_an_empty_catalog() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    assert_eq!(session.commands().unwrap()[0]["name"], "review");
    let native: Value = serde_json::from_str(CATALOG).unwrap();
    for commands in [&native["commands"], &json!([])] {
        std::fs::write(
            root.path().join("agent/catalog-update.json"),
            commands.to_string(),
        )
        .unwrap();
        session
            .start_turn("/fixture-command-update", &spec.config)
            .await
            .unwrap();
        let events = complete(session.as_mut()).await;
        assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
        assert_eq!(
            session.commands().unwrap(),
            config::commands(&json!({"commands":commands})).unwrap()
        );
    }
    session.close().await.unwrap();
    assert!(!root.path().join("agent/sessions").exists());
}

#[tokio::test]
async fn command_queries_use_the_live_execution_lane_without_starting_another_omp() {
    let (root, client, _) = fixture();
    let host = crate::test_support::Fixture::new();
    let (execution, _) = crate::test_support::worker_with_client(
        &host,
        model::creation::Creations::default(),
        Box::new(client),
    );
    let created = crate::test_support::request(
        &execution,
        "agent.create.request",
        json!({"config":{"provider":"omp","cwd":host.cwd}}),
    )
    .await;
    let id = &created["agentId"];
    let calls = root.path().join("agent/calls.jsonl");
    let initial = std::fs::read_to_string(&calls).unwrap();
    for params in [
        json!({"agentId":id}),
        json!({"agentId":id,"draftConfig":{"provider":"omp","cwd":host.cwd}}),
    ] {
        let response =
            crate::test_support::request(&execution, "agent.commands.list.request", params).await;
        assert_eq!(response["commands"][0]["name"], "review");
    }
    assert_eq!(std::fs::read_to_string(&calls).unwrap(), initial);
    std::fs::write(root.path().join("agent/catalog-update.json"), "[]").unwrap();
    crate::test_support::request(
        &execution,
        "agent.message.send.request",
        json!({"agentId":id,"text":"/fixture-command-update"}),
    )
    .await;
    crate::test_support::request(
        &execution,
        "agent.finish.wait.request",
        json!({"agentId":id}),
    )
    .await;
    let before = std::fs::read_to_string(&calls).unwrap();
    let response = crate::test_support::request(
        &execution,
        "agent.commands.list.request",
        json!({"agentId":id}),
    )
    .await;
    assert_eq!(response["commands"], json!([]));
    assert_eq!(std::fs::read_to_string(&calls).unwrap(), before);
    execution.shutdown().await.unwrap();
}
