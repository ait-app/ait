use super::*;

const ADMISSION_DEADLINE: Duration = Duration::from_secs(5);

#[tokio::test]
async fn prompt_admission_uses_its_own_budget() {
    let (directory, mut client, spec) = fixture("");
    client.admission_deadline = Duration::from_secs(60);
    let mut session = client.create_session(&spec).await.unwrap();
    {
        let admission = session.start_turn("gated-admission", &spec.config);
        tokio::pin!(admission);
        tokio::select! {
            result = &mut admission => panic!("prompt completed before release: {result:?}"),
            () = wait_for_prompt_receipt(directory.path()) => {}
        }
        // Freeze time only after startup and the pre-prompt control requests finish.
        tokio::time::pause();
        tokio::time::advance(client.deadline + Duration::from_secs(1)).await;
        // Poll before releasing the peer so a ready response cannot hide an expired timer.
        let pending = futures_util::poll!(&mut admission).is_pending();
        tokio::time::resume();
        assert!(pending, "prompt used the shorter control deadline");
        std::fs::write(directory.path().join("admit-prompt"), "").unwrap();
        admission.await.unwrap();
    }
    assert!(matches!(
        terminal(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn prompt_admission_timeout_never_replays() {
    let (directory, mut client, spec) = fixture("");
    client.deadline = Duration::from_secs(60);
    client.admission_deadline = ADMISSION_DEADLINE;
    let mut session = client.create_session(&spec).await.unwrap();

    timeout_admission(
        session.as_mut(),
        directory.path(),
        &spec.config,
        "never-ack",
    )
    .await;

    assert_failed_admission(session.as_mut(), &spec.config).await;
    let events: Vec<_> = std::iter::from_fn(|| session.poll_turn().unwrap()).collect();
    assert!(matches!(events.as_slice(), [AgentTurnEvent::Failed]));
    assert_eq!(
        session.persistence().unwrap().metadata.unwrap()["hasInput"],
        false
    );
    session.close().await.unwrap();
    assert_one_prompt(directory.path());
}

#[tokio::test]
async fn native_prompt_rejection_keeps_the_session_reusable() {
    let (_directory, client, spec) = fixture("");
    let mut session = client.create_session(&spec).await.unwrap();

    assert_eq!(
        session.start_turn("reject", &spec.config).await,
        Err(AgentSessionError::Rejected)
    );
    assert!(session.poll_turn().unwrap().is_none());
    assert_eq!(
        session.persistence().unwrap().metadata.unwrap()["hasInput"],
        false
    );
    session.start_turn("hello", &spec.config).await.unwrap();
    assert!(matches!(
        terminal(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(Some(text))) if text == "Hello"
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn unknown_disposition_retains_input_and_retires_the_writer() {
    let (directory, client, spec) = fixture("");
    let mut session = client.create_session(&spec).await.unwrap();

    assert_eq!(
        session
            .start_turn("unknown-disposition", &spec.config)
            .await,
        Err(AgentSessionError::Failed)
    );

    let diagnostic = session.failure_diagnostic().unwrap();
    assert_eq!(diagnostic.category, Category::Protocol);
    assert_eq!(diagnostic.source, "disposition");
    let events: Vec<_> = std::iter::from_fn(|| session.poll_turn().unwrap()).collect();
    assert!(matches!(events.as_slice(), [
        AgentTurnEvent::Timeline(entry), AgentTurnEvent::Failed
    ] if entry.item["type"] == "user_message" && entry.item["text"] == "unknown-disposition"));
    assert_eq!(
        session.persistence().unwrap().metadata.unwrap()["hasInput"],
        false
    );
    assert_eq!(
        session.start_turn("retry", &spec.config).await,
        Err(AgentSessionError::Rejected)
    );
    assert_eq!(session.failure_diagnostic(), Some(diagnostic));
    session.close().await.unwrap();
    assert_one_prompt(directory.path());
}

#[tokio::test]
async fn display_limit_without_a_native_file_preserves_the_previous_resume_requirement() {
    for prior_input in [false, true] {
        let (directory, client, spec) = fixture("");
        let mut session = client.create_session(&spec).await.unwrap();
        if prior_input {
            session.start_turn("hello", &spec.config).await.unwrap();
            terminal(session.as_mut()).await;
            std::fs::remove_file(directory.path().join("session.jsonl")).unwrap();
        }

        assert_eq!(
            session.start_turn("handled-limit", &spec.config).await,
            Err(AgentSessionError::Failed)
        );

        assert!(!directory.path().join("session.jsonl").exists());
        assert_eq!(
            session.failure_diagnostic().unwrap().source,
            "display_limit"
        );
        let handle = session.persistence().unwrap();
        assert_eq!(handle.metadata.as_ref().unwrap()["hasInput"], prior_input);
        session.close().await.unwrap();
        let resumed = client
            .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
            .await;
        if prior_input {
            assert!(resumed.is_err());
            assert!(!directory.path().join("session.jsonl").exists());
        } else {
            let mut resumed = resumed.unwrap();
            resumed.start_turn("hello", &spec.config).await.unwrap();
            assert!(matches!(
                terminal(resumed.as_mut()).await.last(),
                Some(AgentTurnEvent::Completed(_))
            ));
            resumed.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn manager_rejection_drops_the_writer_without_projecting_rejected_input() {
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
    use provider::service::agent_manager::AgentManager;
    use provider::storage::timeline::Timeline;
    use provider::testing::Manager;

    let (directory, client, spec) = fixture("");
    let registry = FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json"));
    let timeline = Timeline::open(&directory.path().join("timeline.sqlite")).unwrap();
    let mut manager =
        Manager::new(AgentManager::new(Box::new(registry)).with_timeline(timeline.clone()));
    manager.register(Box::new(client)).unwrap();
    manager.create("agent", &spec).await.unwrap();

    assert!(manager.send("agent", "reject-settled").await.is_err());

    assert!(!manager.has_sessions());
    let (_, rows) = provider::testing::timeline_entries(&timeline, "agent").unwrap();
    assert!(rows.iter().all(|row| row.item["type"] != "user_message"));
    assert_eq!(rows.last().unwrap().item["type"], "error");
    manager.send("agent", "hello").await.unwrap();
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn resume_recovers_history_written_after_the_manager_checkpoint() {
    use domain::agent_runtime::registry::AgentRuntimeRegistry;
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

    use provider::service::agent_manager::AgentManager;
    use provider::testing::Manager;

    for (scenario, rejected) in [
        ("history-on-shutdown", false),
        ("history-on-shutdown", true),
        ("partial-history-on-shutdown", false),
        ("partial-history-on-shutdown", true),
    ] {
        let (directory, client, spec) = fixture(scenario);
        let environment = client.environment.clone();
        let registry = FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json"));
        let mut manager = Manager::new(AgentManager::new(Box::new(registry.clone())));
        manager.register(Box::new(client)).unwrap();
        manager
            .create_with_environment("agent", &spec, &environment)
            .await
            .unwrap();
        manager.send("agent", "handled-before").await.unwrap();
        manager.poll().await.unwrap();
        let before = registry.get("agent").unwrap().unwrap().persistence.unwrap();
        let before = before.metadata.unwrap();
        assert_eq!(before["hasInput"], false);
        assert_eq!(before["observedWork"], true);
        if rejected {
            assert!(manager.send("agent", "reject").await.is_err());
        } else {
            manager.close("agent").await.unwrap();
        }

        assert!(!manager.has_sessions());
        let after = registry.get("agent").unwrap().unwrap().persistence.unwrap();
        let after = after.metadata.unwrap();
        assert_eq!(after["hasInput"], false);
        assert_eq!(after["observedWork"], true);
        let resumed = manager.resume("agent").await.unwrap();
        let resumed = resumed.persistence.unwrap().metadata.unwrap();
        assert_eq!(resumed["hasInput"], true);
        assert!(!resumed.contains_key("observedWork"));
        let arguments: Vec<String> = serde_json::from_str(
            &std::fs::read_to_string(directory.path().join("arguments.json")).unwrap(),
        )
        .unwrap();
        assert!(arguments.iter().any(|argument| argument == "--session"));
        manager.close_all().await.unwrap();
    }
}

#[tokio::test]
async fn prompt_admission_timeout_preserves_observed_work_and_native_history() {
    for command in ["unacknowledged-running", "unacknowledged-settled"] {
        let (directory, mut client, spec) = fixture("");
        client.deadline = Duration::from_secs(60);
        client.admission_deadline = ADMISSION_DEADLINE;
        let mut session = client.create_session(&spec).await.unwrap();

        timeout_admission(session.as_mut(), directory.path(), &spec.config, command).await;

        let handle = session.persistence().unwrap();
        assert_eq!(
            handle.metadata.as_ref().unwrap()["hasInput"],
            true,
            "{command}"
        );
        assert_failed_admission(session.as_mut(), &spec.config).await;
        let events: Vec<_> = std::iter::from_fn(|| session.poll_turn().unwrap()).collect();
        let rows: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                AgentTurnEvent::Timeline(entry) => Some(entry),
                _ => None,
            })
            .collect();
        assert_eq!(rows[0].item["type"], "user_message", "{command}");
        assert_eq!(rows[0].item["text"], command);
        assert_eq!(
            rows.iter()
                .filter(|entry| entry.item["type"] == "user_message")
                .count(),
            1
        );
        assert!(rows.iter().all(|entry| entry.turn_id == rows[0].turn_id));
        assert!(matches!(events.last(), Some(AgentTurnEvent::Failed)));
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, AgentTurnEvent::Failed))
                .count(),
            1
        );
        assert!(!events.iter().any(|event| matches!(
            event,
            AgentTurnEvent::Completed(_) | AgentTurnEvent::Cancelled
        )));
        if command == "unacknowledged-running" {
            assert!(events.iter().any(|event| matches!(
                event, AgentTurnEvent::Timeline(entry)
                    if entry.item["name"] == "bash" && entry.item["status"] == "failed"
            )));
        }
        session.close().await.unwrap();

        let history_path = directory.path().join("session.jsonl");
        let saved_history = std::fs::read_to_string(&history_path).unwrap();
        let mut resumed = client
            .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
            .await
            .unwrap();
        assert_eq!(
            native_thinking(&resumed.runtime_info().await.unwrap()),
            Some("low")
        );
        assert_eq!(
            std::fs::read_to_string(&history_path).unwrap(),
            saved_history
        );
        resumed.close().await.unwrap();
        assert_one_prompt(directory.path());

        std::fs::remove_file(&history_path).unwrap();
        assert!(
            client
                .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
                .await
                .is_err()
        );
        assert!(!history_path.exists());
    }
}

async fn timeout_admission(
    session: &mut dyn AgentSession,
    directory: &std::path::Path,
    config: &StoredAgentConfig,
    command: &str,
) {
    let admission = session.start_turn(command, config);
    tokio::pin!(admission);
    tokio::select! {
        result = &mut admission => panic!("prompt completed before timeout: {result:?}"),
        () = wait_for_prompt_receipt(directory) => {}
    }
    // The fixture's UI response barrier ensures preceding run events are queued.
    // Consume them before advancing the admission deadline.
    assert!(futures_util::poll!(&mut admission).is_pending());
    tokio::time::pause();
    tokio::time::advance(ADMISSION_DEADLINE + Duration::from_secs(1)).await;
    tokio::time::resume();
    // Leave real time for process cleanup, while rejecting use of the 60s control budget.
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(10), admission)
            .await
            .expect("prompt did not honor its admission deadline"),
        Err(AgentSessionError::Failed)
    );
}

async fn wait_for_prompt_receipt(directory: &std::path::Path) {
    let received = directory.join("prompt-received");
    tokio::time::timeout(Duration::from_secs(10), async {
        while !received.exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("fixture did not receive the prompt");
}

async fn assert_failed_admission(session: &mut dyn AgentSession, config: &StoredAgentConfig) {
    let diagnostic = session.failure_diagnostic().unwrap();
    assert_eq!(diagnostic.category, Category::Timeout);
    assert_eq!(diagnostic.source, "prompt_admission");
    assert!(session.start_turn("retry", config).await.is_err());
    assert_eq!(session.failure_diagnostic(), Some(diagnostic));
}

fn assert_one_prompt(directory: &std::path::Path) {
    let log = std::fs::read_to_string(directory.join("requests.jsonl")).unwrap();
    assert_eq!(
        log.lines()
            .filter(|line| serde_json::from_str::<Value>(line).unwrap()["type"] == "prompt")
            .count(),
        1
    );
}
