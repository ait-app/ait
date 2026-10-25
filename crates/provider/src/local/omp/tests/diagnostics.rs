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
        crate::ports::diagnostics::Category::Quota
    );
    let message = session.failure_message().unwrap().into_owned();
    assert!(message.contains("Oh My Pi"));
    assert!(message.contains("insufficient balance"));
    assert!(!message.contains("fixture-secret"));
    session.close().await.unwrap();
    assert_eq!(session.failure_message().as_deref(), Some(message.as_str()));
}

#[tokio::test]
async fn unknown_native_and_adapter_errors_remain_specific_after_close() {
    use crate::ports::diagnostics::Category;

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
        (
            "unknown-tool-result",
            "without a matching tool call",
            Category::Protocol,
        ),
        ("invalid-usage", "usage.input", Category::Protocol),
        ("malformed", "invalid JSON", Category::Protocol),
        (
            "incomplete-frame",
            "incomplete JSON frame",
            Category::Protocol,
        ),
        (
            "invalid-chunks",
            "out-of-order RPC chunks",
            Category::Protocol,
        ),
        (
            "missing-frame-type",
            "without a string type",
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
    let (_, rows) = timeline.read("agent").unwrap();
    assert!(
        rows.iter()
            .any(|row| row.entry.item["text"] == "Hello world")
    );
    assert!(rows.iter().all(|row| row.entry.item["type"] != "error"));
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn native_and_projection_reasons_reach_the_persisted_chat_and_terminal_event() {
    use std::collections::BTreeSet;

    use crate::service::agent_manager::{AgentManager, AgentRegistration};
    use crate::storage::timeline::Timeline;
    use domain::agent_runtime::registry::AgentRuntimeRegistry;
    use model::outbound::{Frame, Outbound};
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

    for (prompt, reason) in [
        ("frame-error", "Upstream shard unavailable"),
        ("invalid-timestamp", "message.timestamp"),
    ] {
        let (root, client, spec) = fixture();
        let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
        let timeline = Timeline::open(&root.path().join("timeline.sqlite")).unwrap();
        let mut manager =
            AgentManager::new(Box::new(registry.clone())).with_timeline(timeline.clone());
        manager.register_client(Box::new(client)).unwrap();
        manager
            .create("agent", &spec, AgentRegistration::default())
            .await
            .unwrap();
        let (outbound, mut output) = Outbound::new();
        let subscription = timeline.events().observe(
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
        let (_, rows) = timeline.read("agent").unwrap();
        let errors: Vec<_> = rows
            .iter()
            .filter(|row| row.entry.item["type"] == "error")
            .collect();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].entry.item["message"], message);
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
            reopened
                .read("agent")
                .unwrap()
                .1
                .iter()
                .any(|row| row.entry.item["message"] == message)
        );
    }
}
