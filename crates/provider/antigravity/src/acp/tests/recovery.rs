use super::*;

#[tokio::test]
async fn an_empty_permission_denial_retains_the_failure_semantics() {
    let (_directory, client, spec) = fixture("");
    let mut session = open(&client, &spec, None).await.unwrap();
    session
        .start_turn("deny-empty", &spec.config)
        .await
        .unwrap();
    let events = finish(&mut session).await;
    assert_eq!(events.last(), Some(&AgentTurnEvent::Failed));
    assert_eq!(
        session.failure_diagnostic().unwrap().category,
        Category::Permission
    );
    session.start_turn("recover", &spec.config).await.unwrap();
    assert!(matches!(
        finish(&mut session).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn legacy_unmarked_handles_resume_directly_over_acp() {
    let (directory, client, spec) = fixture("");
    let legacy =
        acp_session::persistence(PROVIDER, "11111111-2222-4333-8444-555555555555", &spec.cwd);
    assert!(!legacy.metadata.as_ref().unwrap().contains_key("transport"));
    let mut resumed = client
        .resume_session(&legacy, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    let handle = resumed.persistence().unwrap();
    assert_eq!(handle.session_id, legacy.session_id);
    assert_eq!(handle.metadata.as_ref().unwrap()["transport"], "acp");
    resumed
        .start_turn("resumed via acp", &spec.config)
        .await
        .unwrap();
    let events = crate::tests::completed(resumed.as_mut()).await;
    assert_eq!(
        events.last(),
        Some(&AgentTurnEvent::Completed(Some(
            "resumed via acp".to_owned()
        )))
    );
    resumed.close().await.unwrap();
    let frames = logs(&directory);
    assert!(frames.iter().any(|frame| {
        frame["rpc"]["method"] == "session/resume"
            && frame["rpc"]["params"]["sessionId"] == legacy.session_id
    }));
    assert!(frames.iter().all(|frame| {
        frame["args"]
            .as_array()
            .is_none_or(|args| !args.contains(&json!("--input-format")))
    }));
}

#[tokio::test]
async fn supported_releases_open_acp_with_one_model_discovery() {
    for version in ["1.5.15", "1.5.18", "1.6.0", "2.0.0"] {
        let (directory, mut client, spec) = fixture("");
        set_version(&mut client, version);
        assert!(client.is_available().await.unwrap(), "{version}");
        let mut session = client.create_session(&spec).await.unwrap();
        assert_eq!(
            session.persistence().unwrap().metadata.unwrap()["transport"],
            "acp",
            "{version}"
        );
        session
            .start_turn("ACP selected", &spec.config)
            .await
            .unwrap();
        let events = crate::tests::completed(session.as_mut()).await;
        assert_eq!(
            events.last(),
            Some(&AgentTurnEvent::Completed(Some("ACP selected".to_owned())))
        );
        session.close().await.unwrap();
        let frames = logs(&directory);
        assert_eq!(
            frames
                .iter()
                .filter(|frame| frame["args"] == json!(["models"]))
                .count(),
            1,
            "{version}: create_session should query `agy models` once"
        );
        assert_eq!(
            frames
                .iter()
                .filter(|frame| frame["rpc"]["method"] == "session/prompt")
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn old_and_unknown_versions_keep_basic_chat_and_resume_available() {
    for version in ["1.3.0", "1.5.13", "1.5.14", "unknown", "1.5.18-preview"] {
        let (directory, mut client, spec) = fixture("");
        set_version(&mut client, version);
        assert!(client.is_available().await.unwrap(), "{version}");
        let diagnostic = client.diagnostic().await.unwrap();
        assert!(diagnostic.contains("1.5.15"), "{version}: {diagnostic}");
        let mut session = client.create_session(&spec).await.unwrap();
        session
            .start_turn("old CLI text", &spec.config)
            .await
            .unwrap();
        let events = crate::tests::completed(session.as_mut()).await;
        assert_eq!(
            events.last(),
            Some(&AgentTurnEvent::Completed(Some("old CLI text".to_owned())))
        );
        assert!(events.iter().all(|event| !matches!(event, AgentTurnEvent::Timeline(item) if item.item["type"] == "notification")));
        let handle = session.persistence().unwrap();
        session.close().await.unwrap();
        let mut resumed = client
            .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
            .await
            .unwrap();
        resumed.start_turn("continued", &spec.config).await.unwrap();
        assert!(matches!(
            crate::tests::completed(resumed.as_mut()).await.last(),
            Some(AgentTurnEvent::Completed(_))
        ));
        assert_eq!(resumed.persistence().unwrap().session_id, handle.session_id);
        resumed.close().await.unwrap();
        let mut manager = provider::testing::Manager::new(
            provider::service::agent_manager::AgentManager::new(Box::new(
                persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry::new(
                    directory.path().join("agents.json"),
                ),
            )),
        );
        manager.register(Box::new(client)).unwrap();
        let snapshot = manager
            .providers("provider.snapshot.get.request", json!({"cwd":spec.cwd}))
            .await
            .unwrap();
        assert_eq!(snapshot["entries"][0]["status"], "ready", "{version}");
        assert!(logs(&directory).iter().all(|frame| {
            frame["args"]
                .as_array()
                .is_none_or(|args| !args.contains(&json!("--input-format")))
        }));
    }
}

pub(super) fn set_version(client: &mut AntigravityClient, version: &str) {
    let mut environment: BTreeMap<String, String> = client
        .environment
        .entries()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    environment.insert("AGY_ACP_VERSION".to_owned(), version.to_owned());
    client.environment = AgentEnvironment::try_from(environment).unwrap();
}

#[tokio::test]
async fn unsupported_configuration_change_retains_previous_configuration() {
    let (directory, client, spec) = fixture("switch-unsupported");
    let mut session = open(&client, &spec, None).await.unwrap();
    session.start_turn("first", &spec.config).await.unwrap();
    finish(&mut session).await;
    let config = StoredAgentConfig {
        model: Some("plain-test".to_owned()),
        ..spec.config.clone()
    };
    assert_eq!(
        session.start_turn("after switch", &config).await,
        Err(AgentSessionError::Rejected)
    );
    assert_eq!(session.spec.config, spec.config);
    assert!(
        session
            .failure_message()
            .unwrap()
            .contains("previous configuration")
    );
    let frames = logs(&directory);
    assert_eq!(
        frames
            .iter()
            .filter(|frame| frame["rpc"]["method"] == "session/prompt")
            .count(),
        1
    );
    assert!(frames.iter().all(|frame| {
        frame["args"]
            .as_array()
            .is_none_or(|args| !args.contains(&json!("--input-format")))
    }));
    session.start_turn("recovered", &spec.config).await.unwrap();
    finish(&mut session).await;
    session.close().await.unwrap();
}

#[tokio::test]
async fn configuration_failure_retains_old_configuration_and_does_not_submit() {
    let (directory, client, spec) = fixture("unacknowledged-config");
    let mut session = open(&client, &spec, None).await.unwrap();
    let config = StoredAgentConfig {
        thinking_option_id: Some("low".to_owned()),
        ..spec.config.clone()
    };
    assert!(session.start_turn("not admitted", &config).await.is_err());
    assert_eq!(session.spec.config, spec.config);
    assert!(
        session
            .failure_message()
            .unwrap()
            .contains("previous configuration")
    );
    assert!(
        logs(&directory)
            .iter()
            .all(|frame| frame["rpc"]["method"] != "session/prompt")
    );
    session.start_turn("recovered", &spec.config).await.unwrap();
    finish(&mut session).await;
    session.close().await.unwrap();
}

#[tokio::test]
async fn failed_write_never_retries_and_next_explicit_input_resumes() {
    let (directory, client, spec) = fixture("");
    let mut session = open(&client, &spec, None).await.unwrap();
    session.transport.as_mut().unwrap().close().await.unwrap();
    assert!(
        session
            .start_turn("not written", &spec.config)
            .await
            .is_err()
    );
    assert!(session.events.is_empty());
    assert!(session.running.is_none());
    assert!(session.failure_diagnostic().is_some());
    session.start_turn("new input", &spec.config).await.unwrap();
    finish(&mut session).await;
    assert_eq!(
        logs(&directory)
            .iter()
            .filter(|frame| frame["rpc"]["method"] == "session/prompt")
            .count(),
        1
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn cancellation_timeout_and_close_flush_tools_and_leave_one_terminal() {
    for (prompt, close) in [("no-cancel", false), ("wait", true)] {
        let (_directory, client, spec) = fixture("");
        let mut session = open(&client, &spec, None).await.unwrap();
        let turn = session.start_turn(prompt, &spec.config).await.unwrap();
        loop {
            if matches!(next(&mut session).await,AgentTurnEvent::Progress{entry,..} if entry.item["type"] == "tool_call")
            {
                break;
            }
        }
        if close {
            session.close().await.unwrap();
        } else {
            assert!(session.cancel_turn(&turn).await.is_err());
        }
        let events = finish(&mut session).await;
        assert_eq!(
            events.iter().filter(|event| turn::terminal(event)).count(),
            1
        );
        assert!(events.iter().any(|event|matches!(event,AgentTurnEvent::Timeline(item) if item.item["type"] == "tool_call" && item.item["status"] != "running")));
        assert!(session.poll_turn().unwrap().is_none());
        if !close {
            session.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn control_timeout_is_diagnosed_without_headless_retry() {
    let (directory, mut client, spec) = fixture("initialize-timeout");
    client.deadline = Duration::from_secs(2);
    let error = open(&client, &spec, None).await.unwrap_err();
    assert!(matches!(
        error,
        OpenError::Diagnosed {
            diagnostic: Diagnostic {
                category: Category::Timeout,
                ..
            },
            ..
        }
    ));
    assert!(logs(&directory).iter().all(|frame| {
        frame["args"]
            .as_array()
            .is_none_or(|args| !args.contains(&json!("--input-format")))
    }));
}

#[test]
fn persisted_totals_validate_and_do_not_include_context_or_duplicate_reasoning() {
    let mut snapshot = AgentUsage::default();
    let request = usage::request("turn",&json!({"inputTokens":10,"outputTokens":4,"thoughtTokens":2,"cachedReadTokens":5,"cachedWriteTokens":0})).unwrap().unwrap();
    usage::accumulate(&mut snapshot, &request).unwrap();
    usage::accumulate(&mut snapshot, &request).unwrap();
    assert_eq!(snapshot.input_tokens, Some(20));
    assert_eq!(snapshot.output_tokens, Some(8));
    assert_eq!(snapshot.cached_input_tokens, Some(10));
    let mut handle = acp_session::persistence(PROVIDER, "id", "/tmp");
    handle
        .metadata
        .as_mut()
        .unwrap()
        .insert("acpUsage".to_owned(), json!(snapshot));
    assert_eq!(usage::restore(Some(&handle)).unwrap(), snapshot);
    handle
        .metadata
        .as_mut()
        .unwrap()
        .insert("acpUsage".to_owned(), json!({"inputTokens":-1}));
    assert!(usage::restore(Some(&handle)).is_err());
    handle
        .metadata
        .as_mut()
        .unwrap()
        .insert("acpUsage".to_owned(), json!({"contextWindowUsedTokens":1}));
    assert!(usage::restore(Some(&handle)).is_err());
    snapshot.input_tokens = Some(9_007_199_254_740_991);
    let prior = snapshot.clone();
    assert!(usage::accumulate(&mut snapshot, &request).is_err());
    assert_eq!(snapshot, prior);
}
