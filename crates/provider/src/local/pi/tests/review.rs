use super::*;
use crate::ports::diagnostics::Category;

#[tokio::test]
async fn idle_notifications_and_compactions_do_not_poison_later_turns() {
    let (_directory, client, spec) = fixture("idle");
    let mut session = client.create_session(&spec).await.unwrap();
    // Polling idle live agents is part of the normal manager loop.
    for _ in 0..40 {
        assert!(!matches!(
            session.poll_turn().unwrap(),
            Some(AgentTurnEvent::Failed)
        ));
        tokio::task::yield_now().await;
    }
    for _ in 0..2 {
        session.start_turn("hello", &spec.config).await.unwrap();
        let events = terminal(session.as_mut()).await;
        assert!(
            matches!(events.last(), Some(AgentTurnEvent::Completed(Some(text))) if text == "Hello")
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, AgentTurnEvent::Failed))
        );
    }
    session.close().await.unwrap();
}

#[tokio::test]
async fn handled_runs_never_settle_the_next_prompt() {
    for command in [
        "handled",
        "handled-before",
        "handled-after",
        "handled-running",
        "handled-many",
        "handled-deltas",
        "handled-compaction",
    ] {
        let (_directory, client, spec) = fixture("");
        let mut session = client.create_session(&spec).await.unwrap();
        session.start_turn(command, &spec.config).await.unwrap();
        let first = terminal(session.as_mut()).await;
        if matches!(command, "handled" | "handled-compaction") {
            assert!(matches!(
                first.last(),
                Some(AgentTurnEvent::Completed(None))
            ));
        } else {
            assert!(
                matches!(first.last(), Some(AgentTurnEvent::Completed(Some(text))) if text == "Extension"),
                "{command}: {first:?}"
            );
        }
        session.start_turn("hello", &spec.config).await.unwrap();
        let second = terminal(session.as_mut()).await;
        assert!(
            matches!(second.last(), Some(AgentTurnEvent::Completed(Some(text))) if text == "Hello"),
            "{command}: {second:?}"
        );
        assert_eq!(
            second
                .iter()
                .filter(|event| matches!(event, AgentTurnEvent::Completed(_)))
                .count(),
            1
        );
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn handled_run_settled_during_admission_resumes_native_history() {
    let (_directory, client, spec) = fixture("");
    let mut session = client.create_session(&spec).await.unwrap();

    session
        .start_turn("handled-after", &spec.config)
        .await
        .unwrap();
    assert!(matches!(
        terminal(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(Some(text))) if text == "Extension"
    ));
    let handle = session.persistence().unwrap();
    assert_eq!(
        handle
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("hasInput")),
        Some(&Value::Bool(true))
    );
    session.close().await.unwrap();

    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert_eq!(
        resumed.persistence().unwrap().native_handle,
        handle.native_handle
    );
    resumed.close().await.unwrap();
}

#[tokio::test]
async fn native_stderr_and_frame_limits_have_safe_distinct_diagnostics() {
    for (command, category) in [
        ("stderr-auth", Category::Authentication),
        ("oversized", Category::Protocol),
        ("fail", Category::Quota),
    ] {
        let (_directory, client, spec) = fixture("");
        let mut session = client.create_session(&spec).await.unwrap();
        session.start_turn(command, &spec.config).await.unwrap();
        assert!(matches!(
            terminal(session.as_mut()).await.last(),
            Some(AgentTurnEvent::Failed)
        ));
        let diagnostic = session.failure_diagnostic().unwrap();
        assert_eq!(diagnostic.category, category, "{command}");
        if command == "oversized" {
            assert_eq!(diagnostic.source, "frame_limit");
        }
        assert!(!session.failure_message().unwrap().contains("secret-value"));
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn prompt_admission_uses_its_own_budget_and_never_replays_timeout() {
    let (_directory, mut client, spec) = fixture("");
    client.deadline = Duration::from_millis(300);
    client.admission_deadline = Duration::from_secs(2);
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("slow", &spec.config).await.unwrap();
    assert!(matches!(
        terminal(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    session.close().await.unwrap();
    let (directory, mut client, spec) = fixture("");
    client.admission_deadline = Duration::from_millis(100);
    let mut session = client.create_session(&spec).await.unwrap();
    assert!(session.start_turn("never-ack", &spec.config).await.is_err());
    assert_eq!(
        session.failure_diagnostic().unwrap().category,
        Category::Timeout
    );
    assert!(session.start_turn("retry", &spec.config).await.is_err());
    session.close().await.unwrap();
    let log = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    assert_eq!(
        log.lines()
            .filter(|line| serde_json::from_str::<Value>(line).unwrap()["type"] == "prompt")
            .count(),
        1
    );
}

#[tokio::test]
async fn version_errors_are_bounded_and_old_installations_are_actionable() {
    for scenario in [
        "version-malformed",
        "version-oversized",
        "version-exit",
        "version-timeout",
    ] {
        let (_directory, mut client, _spec) = fixture(scenario);
        client.deadline = Duration::from_millis(500);
        assert!(transport::version(&client).await.is_err(), "{scenario}");
    }
    let (_directory, client, _spec) = fixture("old");
    assert!(!client.is_available().await.unwrap());
    assert_eq!(
        client.diagnostic().await.unwrap(),
        "Pi 0.73.1 is unsupported; requires a supported Pi version (1.2 or newer)."
    );
    let unavailable = PiClient::new("/missing/ait-pi-test".into());
    assert!(!unavailable.is_available().await.unwrap());
    assert!(
        unavailable
            .diagnostic()
            .await
            .unwrap()
            .contains("unavailable")
    );
}

#[tokio::test]
async fn catalog_controls_and_runtime_report_native_thinking() {
    let (_directory, client, mut spec) = fixture("");
    let settings = client.settings(&spec.config);
    assert_eq!(settings["availableModes"][0]["id"], "default");
    assert_eq!(settings["capabilities"]["supportsReasoningStream"], true);
    assert_eq!(settings["capabilities"]["supportsSessionListing"], true);
    let details = client.discover(&spec.cwd).await.unwrap();
    assert_eq!(
        details.models[0]["thinkingOptions"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert_eq!(details.models[0]["isDefault"], true);
    let mut session = client.create_session(&spec).await.unwrap();
    assert_eq!(
        native_thinking(&session.runtime_info().await.unwrap()),
        Some("medium")
    );
    session.close().await.unwrap();
    spec.config.model = Some("fixture/missing".into());
    assert_eq!(
        client.validate_selection(&spec).await,
        Err(AgentSessionError::Rejected)
    );
    for scenario in ["bad-models", "bad-model-label"] {
        let (_directory, client, spec) = fixture(scenario);
        assert!(client.discover(&spec.cwd).await.is_err());
    }
}

#[test]
fn malformed_model_definitions_and_directories_are_rejected() {
    for model in [
        "missing".to_owned(),
        "a/b\n".to_owned(),
        format!("a/{}", "x".repeat(511)),
    ] {
        assert!(config::split_model(&model).is_none());
    }
    assert!(config::split_model(&format!("a/{}", "x".repeat(510))).is_some());
    assert!(config::directory("relative").is_err());
    assert!(config::directory("/missing/ait-pi-test").is_err());
    for models in [
        Value::Null,
        json!([{}]),
        json!([{"provider":"a","id":""}]),
        json!([{"provider":"a","id":"b","name":"x".repeat(1025)}]),
        json!([{"provider":"a","id":"b","name":"bad\nlabel"}]),
        json!(vec![json!({"provider":"a","id":"b"}); 4097]),
    ] {
        assert!(config::models(&json!({"models":models}), &Value::Null).is_err());
    }
}

#[test]
fn compaction_lifecycle_and_failure_cleanup_preserve_identity() {
    for (end, expected) in [
        (json!({"result":{}}), "completed"),
        (json!({"aborted":true}), "failed"),
        (json!({"errorMessage":"compaction failed"}), "failed"),
    ] {
        let mut stream = streaming::Stream::default();
        stream.begin("turn".into());
        let start = json!({"type":"compaction_start","reason":"threshold"});
        stream.consume(&start).unwrap();
        stream.consume(&start).unwrap();
        let mut end = end;
        end["type"] = json!("compaction_end");
        end["reason"] = json!("threshold");
        stream.consume(&end).unwrap();
        stream.consume(&end).unwrap();
        let entries: Vec<_> = stream
            .events
            .iter()
            .filter_map(|event| {
                if let AgentTurnEvent::Timeline(entry) = event {
                    Some(entry)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].item["status"], "loading");
        assert_eq!(entries[1].item["status"], expected);
        assert_eq!(
            entries[0].item["compactionId"],
            entries[1].item["compactionId"]
        );
        assert_ne!(entries[0].key, entries[1].key);
    }
    let mut stream = streaming::Stream::default();
    stream.begin("turn".into());
    stream
        .compact(&json!({"type":"compaction_start","reason":"manual"}), false)
        .unwrap();
    stream.consume(&json!({"type":"tool_execution_start","toolCallId":"call","toolName":"bash","args":{"command":"test"}})).unwrap();
    stream.fail();
    assert!(stream.events.iter().any(|event| matches!(event,AgentTurnEvent::Timeline(entry) if entry.item["type"]=="compaction" && entry.item["status"]=="failed" && entry.turn_id.is_none())));
    assert!(stream.events.iter().any(|event| matches!(event,AgentTurnEvent::Timeline(entry) if entry.item["name"]=="bash" && entry.item["status"]=="failed")));
    assert!(matches!(stream.events.back(), Some(AgentTurnEvent::Failed)));

    for has_current_compaction in [false, true] {
        let mut stream = streaming::Stream::default();
        stream.begin("first".into());
        let delta = json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":"x"}});
        while stream.consume(&delta).is_ok() {}
        assert!(
            stream
                .consume(&json!({
                    "type":"compaction_start",
                    "reason":"threshold"
                }))
                .is_err()
        );

        stream.events.clear();
        stream.begin("second".into());
        if has_current_compaction {
            stream
                .consume(&json!({
                    "type":"compaction_start",
                    "reason":"threshold"
                }))
                .unwrap();
        }
        stream.fail();

        let failures: Vec<_> = stream
            .events
            .iter()
            .filter_map(|event| match event {
                AgentTurnEvent::Timeline(entry)
                    if entry.item["type"] == "compaction" && entry.item["status"] == "failed" =>
                {
                    Some(entry)
                }
                _ => None,
            })
            .collect();
        assert_eq!(failures.len(), if has_current_compaction { 2 } else { 1 });
        for entry in &failures {
            assert_eq!(
                entry.key,
                format!(
                    "native:pi:compaction:{}:failed",
                    entry.item["compactionId"].as_str().unwrap()
                )
            );
        }
        if has_current_compaction {
            assert_ne!(failures[0].key, failures[1].key);
        }
    }
}

#[test]
fn event_and_final_text_budgets_cover_tiny_deltas_and_no_delta_messages() {
    let mut stream = streaming::Stream::default();
    stream.begin("turn".into());
    let delta = json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":"x"}});
    assert!((0..300).any(|_| stream.consume(&delta).is_err()));
    stream.fail();
    assert!(stream.events.len() <= 256);
    let mut stream = streaming::Stream::default();
    stream.begin("turn".into());
    let end = json!({"type":"message_end","message":{"role":"assistant","content":vec![json!({"type":"thinking","thinking":"x"});300]}});
    assert!(stream.consume(&end).is_err());
    stream.fail();
    assert!(stream.events.len() <= 256);
    let mut stream = streaming::Stream::default();
    stream.begin("turn".into());
    let end = json!({"type":"message_end","message":{"role":"assistant","content":[{"type":"thinking","thinking":"x".repeat(96*1024)}]}});
    let mut failed = false;
    for _ in 0..90 {
        if stream.consume(&end).is_err() {
            failed = true;
            break;
        }
        stream.events.clear();
    }
    assert!(failed);
}

#[tokio::test]
async fn version_probe_is_cached_and_inbound_frame_boundary_is_inclusive() {
    let (directory, client, spec) = fixture("");
    client.discover(&spec.cwd).await.unwrap();
    client.discover(&spec.cwd).await.unwrap();
    let mut session = client.create_session(&spec).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(directory.path().join("versions.log"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    session
        .start_turn("frame-boundary", &spec.config)
        .await
        .unwrap();
    assert!(matches!(
        terminal(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn malformed_admission_events_are_not_reported_as_display_limits() {
    for shape in ["content", "index", "usage", "compaction", "messages"] {
        let (_directory, client, spec) = fixture("");
        let mut session = client.create_session(&spec).await.unwrap();
        assert!(
            session
                .start_turn(&format!("invalid-admission-{shape}"), &spec.config)
                .await
                .is_err(),
            "{shape}"
        );
        let diagnostic = session.failure_diagnostic().unwrap();
        assert_eq!(diagnostic.category, Category::Protocol, "{shape}");
        assert_eq!(diagnostic.source, "prompt_event", "{shape}");
        assert_eq!(
            session.failure_message(),
            Some(Category::Protocol.message())
        );
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn invalid_handled_settlement_closes_native_writer_without_further_io() {
    let (directory, client, spec) = fixture("");
    let mut session = client.create_session(&spec).await.unwrap();
    assert!(
        session
            .start_turn("handled-invalid-settlement", &spec.config)
            .await
            .is_err()
    );
    assert!(matches!(
        session.poll_turn().unwrap(),
        Some(AgentTurnEvent::Failed)
    ));
    let request_log = directory.path().join("requests.jsonl");
    let requests_before_retry = std::fs::read_to_string(&request_log).unwrap();
    assert!(
        session
            .start_turn("must-not-send", &spec.config)
            .await
            .is_err()
    );
    let requests_after_retry = std::fs::read_to_string(request_log).unwrap();
    assert_eq!(requests_after_retry, requests_before_retry);
    session.close().await.unwrap();
}

#[tokio::test]
async fn admission_display_limit_retains_specific_failure() {
    let (_directory, client, spec) = fixture("");
    let mut session = client.create_session(&spec).await.unwrap();
    assert!(
        session
            .start_turn("handled-limit", &spec.config)
            .await
            .is_err()
    );
    assert_eq!(
        session.failure_diagnostic().unwrap().source,
        "display_limit"
    );
    assert_eq!(
        session.failure_message(),
        Some("The provider output exceeded the display limit.")
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn version_suffixes_are_supported_and_extended_versions_are_actionable() {
    for scenario in ["version-prerelease", "version-build"] {
        let (_directory, client, _spec) = fixture(scenario);
        assert!(transport::version_info(&client).await.unwrap().supported);
    }
    let (_directory, short, _spec) = fixture("version-short");
    assert!(
        short
            .diagnostic()
            .await
            .unwrap()
            .contains("1.2 is unsupported")
    );
    let (_directory, client, _spec) = fixture("version-extended");
    let version = transport::version_info(&client).await.unwrap();
    assert!(!version.supported);
    assert_eq!(version.text, "1.3.0.1");
    assert!(client.diagnostic().await.unwrap().contains("1.3.0.1"));
}

#[tokio::test]
async fn handled_compaction_timeout_closes_uncertain_work() {
    let (_directory, mut client, spec) = fixture("compaction-hang");
    client.admission_deadline = Duration::from_millis(100);
    let mut session = client.create_session(&spec).await.unwrap();
    assert!(
        session
            .start_turn("handled-compaction", &spec.config)
            .await
            .is_err()
    );
    let diagnostic = session.failure_diagnostic().unwrap();
    assert_eq!(diagnostic.category, Category::Timeout);
    assert_eq!(diagnostic.source, "handled_settlement");
    assert!(session.start_turn("hello", &spec.config).await.is_err());
    session.close().await.unwrap();
}

#[tokio::test]
async fn extension_dialogs_cancel_and_unknown_methods_fail_without_admission_timeout() {
    for command in [
        "dialog-select",
        "dialog-confirm",
        "dialog-input",
        "dialog-editor",
        "ui-notifications",
    ] {
        let (directory, client, spec) = fixture("");
        let mut session = client.create_session(&spec).await.unwrap();
        session.start_turn(command, &spec.config).await.unwrap();
        assert!(matches!(
            terminal(session.as_mut()).await.last(),
            Some(AgentTurnEvent::Completed(_))
        ));
        session.close().await.unwrap();
        let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
        let replies: Vec<Value> = requests
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .filter(|value: &Value| value["type"] == "extension_ui_response")
            .collect();
        assert_eq!(replies.len(), usize::from(command != "ui-notifications"));
        assert!(replies.iter().all(|reply| reply["cancelled"] == true));
    }
    let (_directory, client, spec) = fixture("");
    let mut session = client.create_session(&spec).await.unwrap();
    assert!(
        tokio::time::timeout(
            Duration::from_secs(3),
            session.start_turn("dialog-future", &spec.config)
        )
        .await
        .expect("unknown UI method must fail immediately")
        .is_err()
    );
    let diagnostic = session.failure_diagnostic().unwrap();
    assert_eq!(diagnostic.category, Category::Protocol);
    assert_eq!(diagnostic.source, "extension_method");
    session.close().await.unwrap();
}
