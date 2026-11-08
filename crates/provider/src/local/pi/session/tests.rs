use super::*;
use crate::local::pi::tests::fixture;

#[tokio::test]
async fn persisted_work_requires_native_activity_and_a_matching_header() {
    let (directory, client, spec) = fixture("");
    let mut session = open(&client, &spec, None).await.unwrap();
    let saved = directory.path().join("saved.jsonl");
    let header = json!({"type":"session","version":3,"id":session.id,"cwd":spec.cwd});
    std::fs::write(&saved, format!("{header}\n")).unwrap();
    let wrong_extension = saved.with_extension("txt");
    std::fs::write(&wrong_extension, format!("{header}\n")).unwrap();
    let missing = directory.path().join("missing.jsonl");

    for initial_state in [
        PersistenceState::Empty,
        PersistenceState::Observed,
        PersistenceState::Required,
    ] {
        for (path, is_file) in [
            (Value::Null, false),
            (json!(42), false),
            (json!(missing), false),
            (json!(directory.path()), false),
            (json!(wrong_extension), false),
            (json!(saved), true),
        ] {
            session.state["sessionFile"] = path;

            session.persistence_state = initial_state;
            session.observe_persisted_work().await;

            assert_eq!(
                session.persistence_state == PersistenceState::Required,
                initial_state == PersistenceState::Required
                    || (initial_state == PersistenceState::Observed && is_file),
                "initial_state={initial_state:?}, path={}",
                session.state["sessionFile"]
            );
        }
    }
    session.close().await.unwrap();
}

#[tokio::test]
async fn shutdown_error_still_promotes_history_written_during_native_cleanup() {
    for scenario in ["history-on-shutdown", "partial-history-on-shutdown"] {
        let (_directory, client, spec) = fixture(scenario);
        let mut session = open(&client, &spec, None).await.unwrap();
        session
            .start_turn("handled-before", &spec.config)
            .await
            .unwrap();
        assert_eq!(session.persistence_state, PersistenceState::Observed);
        // The request layer may already have closed Pi before session cleanup runs.
        session.transport.close().await.unwrap();
        transport::tests::fail_completed_shutdown(&mut session.transport);

        assert_eq!(session.close().await, Err(AgentSessionError::Failed));

        assert_eq!(session.persistence_state, PersistenceState::Required);
        let metadata = session.persistence().unwrap().metadata.unwrap();
        assert_eq!(metadata["hasInput"], true);
        assert!(!metadata.contains_key("observedWork"));
        session.close().await.unwrap();
        assert_eq!(session.persistence_state, PersistenceState::Required);
    }
}

#[tokio::test]
async fn observed_resume_uses_the_same_path_rules_as_saved_history() {
    let (directory, client, spec) = fixture("");
    let mut session = open(&client, &spec, None).await.unwrap();
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let header = json!({"type":"session","version":3,"id":handle.session_id,"cwd":spec.cwd});

    for (filename, native_path, required) in [
        ("native.jsonl", true, true),
        ("metadata.jsonl", false, false),
        ("notes.txt", true, false),
    ] {
        let path = directory.path().join(filename);
        std::fs::write(&path, format!("{header}\n")).unwrap();
        let mut saved = handle.clone();
        let metadata = saved.metadata.as_mut().unwrap();
        metadata.insert("observedWork".into(), json!(true));
        if native_path {
            saved.native_handle = Some(json!(path));
        } else {
            saved.native_handle = None;
            metadata.insert("sessionFile".into(), json!(path));
        }

        let mut resumed = open(&client, &spec, Some(&saved)).await.unwrap();

        assert_eq!(
            resumed.persistence_state == PersistenceState::Required,
            required,
            "{filename}"
        );
        let arguments: Vec<String> = serde_json::from_str(
            &std::fs::read_to_string(directory.path().join("arguments.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            arguments.iter().any(|argument| argument == "--session"),
            required
        );
        resumed.close().await.unwrap();
    }
}

#[tokio::test]
async fn handled_admission_rechecks_previously_unsaved_work() {
    let (directory, client, spec) = fixture("history-on-shutdown");
    let mut session = open(&client, &spec, None).await.unwrap();
    session
        .start_turn("handled-before", &spec.config)
        .await
        .unwrap();
    assert_eq!(session.persistence_state, PersistenceState::Observed);
    let header = json!({"type":"session","version":3,"id":session.id,"cwd":spec.cwd});
    std::fs::write(
        directory.path().join("session.jsonl"),
        format!("{header}\n"),
    )
    .unwrap();

    session.start_turn("handled", &spec.config).await.unwrap();

    assert_eq!(session.persistence_state, PersistenceState::Required);
    assert_eq!(
        session.persistence().unwrap().metadata.unwrap()["hasInput"],
        true
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn handled_work_with_a_foreign_header_does_not_require_native_resume() {
    let (directory, client, spec) = fixture("");
    let mut session = open(&client, &spec, None).await.unwrap();
    let foreign = directory.path().join("foreign.jsonl");
    let header = json!({"type":"session","version":3,"id":Uuid::new_v4(),"cwd":spec.cwd});
    std::fs::write(&foreign, format!("{header}\n")).unwrap();
    std::fs::write(
        directory.path().join("state-overrides.json"),
        json!({"sessionFile":foreign}).to_string(),
    )
    .unwrap();

    session
        .start_turn("handled-before", &spec.config)
        .await
        .unwrap();

    assert!(session.stream.settled);
    assert_ne!(session.persistence_state, PersistenceState::Required);
    let handle = session.persistence().unwrap();
    assert_eq!(handle.metadata.as_ref().unwrap()["observedWork"], true);
    session.close().await.unwrap();
    let mut resumed = open(&client, &spec, Some(&handle)).await.unwrap();
    assert_eq!(resumed.persistence_state, PersistenceState::Observed);
    let arguments: Vec<String> = serde_json::from_str(
        &std::fs::read_to_string(directory.path().join("arguments.json")).unwrap(),
    )
    .unwrap();
    assert!(arguments.iter().any(|argument| argument == "--session-id"));
    resumed.close().await.unwrap();
}

#[tokio::test]
async fn rejected_prompt_does_not_borrow_concurrent_native_work() {
    for command in ["reject-running", "reject-settled"] {
        let (_directory, client, spec) = fixture("");
        let mut session = open(&client, &spec, None).await.unwrap();

        assert_eq!(
            session.start_turn(command, &spec.config).await,
            Err(AgentSessionError::Rejected)
        );

        assert_ne!(session.persistence_state, PersistenceState::Required);
        assert!(!session.closed);
        assert!(!session.stream.admission);
        assert!(session.stream.events().iter().all(|event| !matches!(event,
            AgentTurnEvent::Timeline(entry) if entry.item["type"] == "user_message"
        )));
        assert!(session.stream.events().iter().any(|event| matches!(event,
            AgentTurnEvent::Timeline(entry) | AgentTurnEvent::Progress { entry, .. }
                if entry.item["type"] == "tool_call"
        )));
        assert!(Path::new(session.state["sessionFile"].as_str().unwrap()).is_file());
        let next = session.start_turn("hello", &spec.config).await;
        if command == "reject-running" {
            assert_eq!(next, Err(AgentSessionError::Busy));
        } else {
            assert!(next.is_ok());
        }
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn admission_exits_restore_polling_budgets() {
    for command in [
        "hello",
        "handled",
        "reject",
        "handled-invalid-settlement",
        "handled-limit",
        "unknown-disposition",
    ] {
        let (_directory, client, spec) = fixture("");
        let mut session = open(&client, &spec, None).await.unwrap();

        let result = session.start_turn(command, &spec.config).await;

        assert!(!session.stream.admission, "{command}");
        if matches!(command, "hello" | "handled") {
            assert!(result.is_ok(), "{command}");
        } else if command == "reject" {
            assert_eq!(result, Err(AgentSessionError::Rejected));
            assert!(!session.closed);
        } else {
            assert_eq!(result, Err(AgentSessionError::Failed), "{command}");
            assert!(session.closed, "{command}");
        }
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn input_and_handled_display_limits_end_admission_and_retire_the_writer() {
    // With one new input row and two cleanup slots reserved, 4093 queued rows
    // leave no room for handled completion; 4094 leave no room for the input.
    for backlog_size in [4093, 4094] {
        let (_directory, client, spec) = fixture("");
        let mut session = open(&client, &spec, None).await.unwrap();
        session.stream.admission = true;
        let backlog = (0..backlog_size)
            .flat_map(|index| Stream::user_entries(&index.to_string(), None, "previous", &[]))
            .collect();
        session.stream.insert_submitted(backlog).unwrap();

        assert_eq!(
            session.start_turn("handled", &spec.config).await,
            Err(AgentSessionError::Failed)
        );

        assert!(!session.stream.admission);
        assert!(session.closed);
        assert!(session.active.is_none());
        assert_eq!(
            session.failure_diagnostic().unwrap().source,
            "display_limit"
        );
        let events: Vec<_> = std::iter::from_fn(|| session.stream.pop()).collect();
        assert!(matches!(events.last(), Some(AgentTurnEvent::Failed)));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, AgentTurnEvent::Completed(_)))
        );
        let inputs = events
            .iter()
            .filter(|event| {
                matches!(event,
                    AgentTurnEvent::Timeline(entry) if entry.item["text"] == "handled"
                )
            })
            .count();
        assert_eq!(inputs, usize::from(backlog_size == 4093));
        let delta = json!({"type":"message_update","assistantMessageEvent":{
            "type":"text_delta","contentIndex":0,"delta":"x"
        }});
        assert!((0..256).any(|_| session.stream.consume(&delta).is_err()));
        assert_eq!(
            session.start_turn("retry", &spec.config).await,
            Err(AgentSessionError::Rejected)
        );
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn display_limit_rechecks_deferred_history_after_shutdown() {
    for scenario in ["history-on-shutdown", "partial-history-on-shutdown"] {
        let (_directory, client, spec) = fixture(scenario);
        let mut session = open(&client, &spec, None).await.unwrap();
        session.stream.admission = true;
        let backlog = (0..4094)
            .flat_map(|index| Stream::user_entries(&index.to_string(), None, "previous", &[]))
            .collect();
        session.stream.insert_submitted(backlog).unwrap();

        assert_eq!(
            session.start_turn("handled-running", &spec.config).await,
            Err(AgentSessionError::Failed)
        );

        assert!(session.closed);
        assert_eq!(
            session.persistence_state,
            PersistenceState::Required,
            "{scenario}"
        );
        assert_eq!(
            session.failure_diagnostic().unwrap().source,
            "display_limit"
        );
        let handle = session.persistence().unwrap();
        let mut resumed = open(&client, &spec, Some(&handle)).await.unwrap();
        resumed.close().await.unwrap();
    }
}

#[tokio::test]
async fn timed_out_admission_preserves_its_diagnostic_when_input_cannot_fit() {
    for command in ["unacknowledged-running", "unacknowledged-settled"] {
        let (directory, mut client, spec) = fixture("");
        client.deadline = Duration::from_secs(60);
        client.admission_deadline = Duration::from_secs(5);
        let mut session = open(&client, &spec, None).await.unwrap();
        session.stream.admission = true;
        // Native tool progress and settlement still fit, but the submitted input
        // would exceed the admission cap after reserving terminal cleanup.
        let backlog = (0..4092)
            .flat_map(|index| Stream::user_entries(&index.to_string(), None, "previous", &[]))
            .collect();
        session.stream.insert_submitted(backlog).unwrap();

        {
            let admission = session.start_turn(command, &spec.config);
            tokio::pin!(admission);
            tokio::select! {
                result = &mut admission => panic!("prompt completed before timeout: {result:?}"),
                () = async {
                    tokio::time::timeout(Duration::from_secs(10), async {
                        while !directory.path().join("prompt-received").exists() {
                            tokio::time::sleep(Duration::from_millis(5)).await;
                        }
                    }).await.expect("fixture did not receive the prompt");
                } => {}
            }
            // The fixture's response barrier queues native activity before this point.
            assert!(futures_util::poll!(&mut admission).is_pending());
            tokio::time::pause();
            tokio::time::advance(client.admission_deadline + Duration::from_secs(1)).await;
            tokio::time::resume();
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(10), admission)
                    .await
                    .unwrap(),
                Err(AgentSessionError::Failed)
            );
        }

        let diagnostic = session.failure_diagnostic().unwrap();
        assert_eq!(diagnostic.category, Category::Timeout, "{command}");
        assert_eq!(diagnostic.source, "prompt_admission");
        assert_eq!(
            session.failure_message().as_deref(),
            Some(Category::Timeout.message())
        );
        assert!(session.closed);
        assert!(!session.stream.admission);
        assert_eq!(session.persistence_state, PersistenceState::Required);
        let events: Vec<_> = std::iter::from_fn(|| session.stream.pop()).collect();
        assert_eq!(events.len(), 4094);
        assert!(matches!(events.last(), Some(AgentTurnEvent::Failed)));
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, AgentTurnEvent::Failed))
                .count(),
            1
        );
        assert!(!events.iter().any(|event| matches!(event,
            AgentTurnEvent::Timeline(entry) if entry.item["text"] == command
        )));
        assert_eq!(
            session.start_turn("retry", &spec.config).await,
            Err(AgentSessionError::Rejected)
        );
        assert_eq!(session.failure_diagnostic(), Some(diagnostic));
        session.close().await.unwrap();
    }
}
