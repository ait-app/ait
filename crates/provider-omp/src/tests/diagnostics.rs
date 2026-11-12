use super::*;

#[tokio::test]
async fn native_errors_retain_detail_after_close_and_successful_retries_clear_it() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("native-error-recovered", &spec.config)
        .await
        .unwrap();
    assert!(matches!(
        complete(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    assert_eq!(session.failure_message(), None);
    assert_eq!(session.failure_diagnostic(), None);
    session
        .start_turn("native-error", &spec.config)
        .await
        .unwrap();
    assert_eq!(
        complete(session.as_mut()).await.last(),
        Some(&AgentTurnEvent::Failed)
    );
    assert_eq!(
        session.failure_diagnostic().unwrap().category,
        provider_core::ports::diagnostics::Category::Quota
    );
    let message = session.failure_message().unwrap().into_owned();
    assert!(message.contains("Oh My Pi"));
    assert!(message.contains("insufficient balance"));
    assert!(!message.contains("fixture-secret"));
    // D27: Paseo's detail, with only the first 500 characters of partial output.
    for detail in [
        "stopReason=error",
        "model=test/model",
        "responseModel=test-model-2026",
        "responseId=resp_1",
    ] {
        assert!(message.contains(detail), "{detail}: {message}");
    }
    let partial = format!("partial=\"Partial {}\"", "x".repeat(492));
    assert!(message.contains(&partial), "{message}");
    assert!(!message.contains(&"x".repeat(493)), "{message}");
    session.close().await.unwrap();
    assert_eq!(session.failure_message().as_deref(), Some(message.as_str()));
}

#[tokio::test]
async fn unknown_native_and_adapter_errors_remain_specific_after_close() {
    use provider_core::ports::diagnostics::Category;

    for (prompt, reason, category) in [
        (
            "unknown-native-error",
            "Upstream shard unavailable",
            Category::Unknown,
        ),
        (
            "frame-error",
            "Upstream shard unavailable",
            Category::Unknown,
        ),
        ("invalid-timestamp", "message.timestamp", Category::Protocol),
        ("invalid-usage", "usage.input", Category::Protocol),
        (
            "incomplete-frame",
            "incomplete JSON frame",
            Category::Protocol,
        ),
        ("closed", "process or connection closed", Category::Closed),
    ] {
        let (_root, client, spec) = fixture();
        let mut session = client.create_session(&spec).await.unwrap();
        session.start_turn(prompt, &spec.config).await.unwrap();
        loop {
            match next(session.as_mut()).await {
                Err(AgentSessionError::Failed) | Ok(AgentTurnEvent::Failed) => break,
                Ok(AgentTurnEvent::Completed(_) | AgentTurnEvent::Cancelled) => {
                    panic!("{prompt} completed instead of failing");
                }
                Ok(_) => {}
                Err(error) => panic!("unexpected failure: {error}"),
            }
        }
        let diagnostic = session.failure_diagnostic().unwrap();
        assert_eq!(diagnostic.category, category, "{prompt}");
        let message = session.failure_message().unwrap().into_owned();
        assert!(message.contains(reason), "{prompt}: {message}");
        assert!(!message.contains("fixture-secret"));
        if prompt == "frame-error" {
            assert!(message.contains("HTTP 503"));
        }
        session.close().await.unwrap();
        assert_eq!(session.failure_message().as_deref(), Some(message.as_str()));
    }
}

#[tokio::test]
async fn invalid_frames_are_dropped_without_failing_the_turn() {
    for prompt in [
        "garbage-line",
        "non-object-line",
        "invalid-chunks",
        "uncorrelated-failure",
        "unknown-tool-result",
    ] {
        let (_root, client, spec) = fixture();
        let mut session = client.create_session(&spec).await.unwrap();
        session.start_turn(prompt, &spec.config).await.unwrap();

        let events = complete(session.as_mut()).await;
        assert!(
            matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
            "{prompt}"
        );
        if prompt == "unknown-tool-result" {
            // O13: Paseo shows a result without a known call under its native name.
            assert!(
                events
                    .iter()
                    .any(|event| matches!(event, AgentTurnEvent::Timeline(entry)
                if entry.item["callId"] == "unknown" && entry.item["name"] == "bash"
                    && entry.item["status"] == "failed"))
            );
        }
        assert_eq!(session.failure_diagnostic(), None, "{prompt}");
        session.close().await.unwrap();
        assert_eq!(session.failure_message(), None, "{prompt}");
    }
}

#[tokio::test]
async fn an_unexpected_exit_reports_its_code_and_a_redacted_stderr_tail() {
    use provider_core::ports::diagnostics::Category;

    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("crash", &spec.config).await.unwrap();
    let mut terminal = None;
    while let Ok(event) = next(session.as_mut()).await {
        assert!(!matches!(event, AgentTurnEvent::Completed(_)));
        terminal = Some(event);
    }
    // D28: the exit ends the turn as a reported failure before the connection error.
    assert_eq!(terminal, Some(AgentTurnEvent::Failed));

    assert_eq!(
        session.failure_diagnostic().unwrap().category,
        Category::Closed
    );
    let message = session.failure_message().unwrap().into_owned();
    assert!(
        message.starts_with("Oh My Pi: OMP exited unexpectedly with code 3."),
        "{message}"
    );
    assert!(message.contains("fatal: worker crashed"), "{message}");
    assert!(message.contains("https://example.com"), "{message}");
    assert!(
        message.contains("process or connection closed"),
        "{message}"
    );
    assert!(!message.contains("fixture-secret"), "{message}");
    assert!(!message.contains("/private/path"), "{message}");
    assert!(message.len() < 4096, "{}", message.len());
    session.close().await.unwrap();
    assert_eq!(session.failure_message().as_deref(), Some(message.as_str()));
}

#[tokio::test]
async fn a_signalled_exit_reports_its_signal() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("killed", &spec.config).await.unwrap();
    while next(session.as_mut()).await.is_ok() {}

    let message = session.failure_message().unwrap().into_owned();
    assert!(
        message.contains("stopped unexpectedly by signal 9"),
        "{message}"
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn background_command_timeout_does_not_become_a_provider_failure() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("background-tool-error", &spec.config)
        .await
        .unwrap();

    assert!(matches!(
        complete(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    assert_eq!(session.failure_diagnostic(), None);
    assert_eq!(session.failure_message(), None);
    session.close().await.unwrap();
}

#[tokio::test]
async fn late_background_updates_do_not_fail_the_persisted_turn() {
    use domain::agent_runtime::{AgentRuntimeStatus, registry::AgentRuntimeRegistry};
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
    use provider::service::agent_manager::AgentManager;
    use provider::storage::timeline::Timeline;
    use provider::testing::Manager;

    let (root, client, spec) = fixture();
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    let timeline = Timeline::open(&root.path().join("timeline.sqlite")).unwrap();
    let mut manager =
        Manager::new(AgentManager::new(Box::new(registry.clone())).with_timeline(timeline.clone()));
    manager.register(Box::new(client)).unwrap();
    manager.create("agent", &spec).await.unwrap();
    let handle = registry.get("agent").unwrap().unwrap().persistence;
    for prompt in ["background-tool-error", "background-busy", "continue"] {
        manager.send("agent", prompt).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while manager.active_turn("agent").is_some() {
                manager.poll().await.unwrap();
                tokio::time::sleep(Duration::from_millis(2)).await;
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
        assert_eq!(record.last_error, None);
        assert_eq!(
            record.persistence.as_ref().map(|value| &value.session_id),
            handle.as_ref().map(|value| &value.session_id)
        );
        assert!(manager.live_snapshot("agent").is_some());
    }
    let (_, rows) = provider::testing::timeline_entries(&timeline, "agent").unwrap();
    assert!(rows.iter().any(|row| row.item["text"] == "Hello world"));
    assert!(rows.iter().all(|row| row.item["type"] != "error"));
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn native_and_projection_reasons_reach_the_persisted_chat_and_terminal_event() {
    use std::collections::BTreeSet;

    use domain::agent_runtime::registry::AgentRuntimeRegistry;
    use model::outbound::{Frame, Outbound};
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
    use provider::service::agent_manager::AgentManager;
    use provider::storage::timeline::Timeline;
    use provider::testing::Manager;

    for (prompt, reason) in [
        ("frame-error", "Upstream shard unavailable"),
        ("invalid-timestamp", "message.timestamp"),
    ] {
        let (root, client, spec) = fixture();
        let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
        let timeline = Timeline::open(&root.path().join("timeline.sqlite")).unwrap();
        let mut manager = Manager::new(
            AgentManager::new(Box::new(registry.clone())).with_timeline(timeline.clone()),
        );
        manager.register(Box::new(client)).unwrap();
        manager.create("agent", &spec).await.unwrap();
        let (outbound, mut output) = Outbound::new();
        let subscription = provider::testing::timeline_events(&timeline).observe(
            "test".to_owned(),
            BTreeSet::from(["agent".to_owned()]),
            outbound,
        );
        subscription.activate().unwrap();
        manager.send("agent", prompt).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while manager.active_turn("agent").is_some() {
                manager.poll().await.unwrap();
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();

        let message = registry.get("agent").unwrap().unwrap().last_error.unwrap();
        assert!(message.contains(reason), "{message}");
        assert!(!message.contains("fixture-secret"));
        let (_, rows) = provider::testing::timeline_entries(&timeline, "agent").unwrap();
        let errors: Vec<_> = rows
            .iter()
            .filter(|row| row.item["type"] == "error")
            .collect();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].item["message"], message);
        let mut terminal = None;
        while let Ok(frame) = output.try_recv() {
            if let Frame::Text(bytes) = frame.message {
                let event: Value = serde_json::from_str(&bytes).unwrap();
                if event["params"]["event"]["type"] == "turn_failed" {
                    terminal = Some(event["params"]["event"]["error"].clone());
                }
            }
        }
        assert_eq!(terminal.unwrap(), message);
        manager.close_all().await.unwrap();
        drop(manager);
        drop(timeline);
        let reopened = Timeline::open(&root.path().join("timeline.sqlite")).unwrap();
        assert!(
            provider::testing::timeline_entries(&reopened, "agent")
                .unwrap()
                .1
                .iter()
                .any(|row| row.item["message"] == message)
        );
    }
}

#[tokio::test]
async fn a_process_exit_fails_the_turn_and_the_runtime_relaunches_the_session_file() {
    use domain::agent_runtime::{AgentRuntimeStatus, registry::AgentRuntimeRegistry};
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
    use provider::service::agent_manager::AgentManager;
    use provider::testing::Manager;

    let (root, client, spec) = fixture();
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    let mut manager = Manager::new(AgentManager::new(Box::new(registry.clone())));
    manager.register(Box::new(client)).unwrap();
    manager.create("agent", &spec).await.unwrap();
    for (prompt, status) in [
        ("crash", AgentRuntimeStatus::Error),
        ("next", AgentRuntimeStatus::Idle),
    ] {
        manager.send("agent", prompt).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while manager.active_turn("agent").is_some() {
                manager.poll().await.unwrap();
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
        let record = registry.get("agent").unwrap().unwrap();
        assert_eq!(record.last_status, status, "{prompt}");
        if prompt == "crash" {
            let reason = record.last_error.unwrap();
            assert!(
                reason.contains("exited unexpectedly with code 3"),
                "{reason}"
            );
        }
    }
    manager.close_all().await.unwrap();
    let launched = launches(&calls_after(root.path(), 0));
    assert_eq!(launched.len(), 2);
    let file = registry
        .get("agent")
        .unwrap()
        .unwrap()
        .persistence
        .unwrap()
        .metadata
        .unwrap()["sessionFile"]
        .as_str()
        .unwrap()
        .to_owned();
    // The runtime closed the failed session and resumed its file for the next prompt.
    assert!(launched[1].ends_with(&["--session".to_owned(), file]));
}

#[tokio::test]
async fn the_diagnostic_reports_version_support_and_roots_without_a_session_process() {
    let (root, client, _spec) = fixture_with(&[
        ("OMP_PROFILE", ""),
        ("XDG_DATA_HOME", ""),
        ("XDG_STATE_HOME", ""),
        ("XDG_CACHE_HOME", ""),
        ("ANTHROPIC_API_KEY", "fixture-secret"),
    ]);
    let agent = root.path().join("agent");

    let report = client.diagnostic().await.unwrap();

    let program = client.program.to_string_lossy();
    for row in [
        "Oh My Pi (OMP)".to_owned(),
        format!("  Binary: {program}"),
        "  Version: omp/18.18.8".to_owned(),
        "  Version support: 18.18.8 (supported; minimum 18.12.6)".to_owned(),
        "  Active profile: default".to_owned(),
        format!("  Agent directory: {}", agent.display()),
        format!(
            "  Agent database: {} (not found)",
            agent.join("agent.db").display()
        ),
    ] {
        assert!(report.lines().any(|line| line == row), "{row}: {report}");
    }
    for label in [
        "Config root",
        "XDG data root",
        "XDG state root",
        "XDG cache root",
    ] {
        assert!(
            report.contains(&format!("\n  {label}: /")),
            "{label}: {report}"
        );
    }
    assert!(!report.contains("fixture-secret"));
    assert!(report.len() <= 4096);
    // `--version` starts no RPC session and creates no OMP state.
    assert!(!agent.exists());

    std::fs::create_dir_all(&agent).unwrap();
    std::fs::write(agent.join("agent.db"), "").unwrap();
    assert!(
        client
            .diagnostic()
            .await
            .unwrap()
            .contains("agent.db (found)")
    );
}

#[tokio::test]
async fn the_diagnostic_names_an_old_release_a_failed_version_and_an_invalid_profile() {
    for (variables, expected) in [
        (
            &[("OMP_FIXTURE_VERSION", "18.12.5")][..],
            "  Version support: 18.12.5 (unsupported; minimum 18.12.6)",
        ),
        (
            &[("OMP_FIXTURE_VERSION", "dev")][..],
            "  Version support: unknown (minimum 18.12.6)",
        ),
        (
            &[("OMP_FIXTURE_VERSION_FAILS", "1")][..],
            "  Version: unavailable (exited unsuccessfully)",
        ),
        (
            &[("OMP_PROFILE", "Bad/Profile")][..],
            "  Active profile: invalid OMP_PROFILE or PI_PROFILE; OMP refuses to start",
        ),
    ] {
        let (_root, client, _spec) = fixture_with(variables);
        let report = client.diagnostic().await.unwrap();
        assert!(report.lines().any(|line| line == expected), "{report}");
    }
}
