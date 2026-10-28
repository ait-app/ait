use super::*;

#[tokio::test]
async fn configuration_storage_failures_do_not_abort_poll_or_skip_native_cleanup() {
    let (mut manager, registry, client) = make_manager();
    for id in ["agent-1", "agent-2"] {
        manager
            .create(id, &spec(), AgentRegistration::default())
            .await
            .unwrap();
        manager.live.get_mut(id).unwrap().session_use =
            SessionUse::Configuration(AgentRuntimeStatus::Idle);
    }
    registry.0.lock().unwrap().fail_update = true;

    for attempt in 1..=2 {
        // Match the command lane's pre-dispatch preparation.
        manager.reconcile().await.unwrap();
        manager.poll().await.unwrap();
        assert_eq!(client.0.lock().unwrap().close_calls, attempt * 4);
        assert!(manager.live.is_empty());
        assert_eq!(manager.pending_configuration_cleanup.len(), 2);
        assert_eq!(manager.session_budget.available_permits(), 32);
    }

    registry.0.lock().unwrap().fail_update = false;
    manager.poll().await.unwrap();
    assert!(manager.live.is_empty());
    assert!(manager.pending_configuration_cleanup.is_empty());
    assert!(
        registry
            .list()
            .unwrap()
            .iter()
            .all(|record| { record.last_status == AgentRuntimeStatus::Idle })
    );
}

#[tokio::test]
async fn failed_native_cleanup_does_not_block_other_cleanup_or_interactive_commands() {
    let (mut manager, _, _) = make_manager();
    for id in ["agent-1", "agent-2", "interactive"] {
        manager
            .create(id, &spec(), AgentRegistration::default())
            .await
            .unwrap();
        if id != "interactive" {
            manager.live.get_mut(id).unwrap().session_use =
                SessionUse::Configuration(AgentRuntimeStatus::Idle);
        }
    }
    let failing = Arc::new(Mutex::new(FakeState {
        fail_close: true,
        ..FakeState::default()
    }));
    manager.live.get_mut("agent-1").unwrap().session = Box::new(FakeSession(failing.clone()));

    manager.reconcile().await.unwrap();
    manager.poll().await.unwrap();
    assert!(!manager.live.contains_key("agent-1"));
    assert!(
        manager
            .pending_configuration_cleanup
            .contains_key("agent-1")
    );
    assert!(!manager.live.contains_key("agent-2"));
    manager.send("interactive", "hello").await.unwrap();
    manager.reconcile().await.unwrap();
    manager.poll().await.unwrap();
    manager.cancel("interactive").await.unwrap();
    manager.poll().await.unwrap();
    assert!(manager.active_turn("interactive").is_none());

    failing.lock().unwrap().fail_close = false;
    manager.poll().await.unwrap();
    assert!(!manager.live.contains_key("agent-1"));
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn failed_configuration_cleanup_releases_global_capacity_and_resume_rebuilds() {
    let (mut manager, _, client) = make_manager();
    for index in 0..32 {
        let id = format!("agent-{index}");
        manager
            .create(&id, &spec(), AgentRegistration::default())
            .await
            .unwrap();
        manager.live.get_mut(&id).unwrap().session_use =
            SessionUse::Configuration(AgentRuntimeStatus::Idle);
    }
    client.0.lock().unwrap().fail_close = true;

    manager.poll().await.unwrap();

    assert!(manager.live.is_empty());
    assert_eq!(manager.pending_configuration_cleanup.len(), 32);
    assert_eq!(manager.session_budget.available_permits(), 32);
    manager
        .create("new-agent", &spec(), AgentRegistration::default())
        .await
        .unwrap();
    assert_eq!(
        manager.resume("agent-0").await,
        Err(AgentManagerError::Session)
    );
    assert!(client.0.lock().unwrap().resume_purposes.is_empty());

    client.0.lock().unwrap().fail_close = false;
    manager.resume("agent-0").await.unwrap();
    assert_eq!(
        client.0.lock().unwrap().resume_purposes,
        vec![AgentResumePurpose::Interactive]
    );
    assert!(manager.live.contains_key("agent-0"));
    assert!(
        !manager
            .pending_configuration_cleanup
            .contains_key("agent-0")
    );
    manager.close_all().await.unwrap();
    assert!(manager.pending_configuration_cleanup.is_empty());
    assert_eq!(manager.session_budget.available_permits(), 32);
}

#[tokio::test]
async fn detached_configuration_cleanup_preserves_pending_runtime_for_retry() {
    for queued_event in [false, true] {
        let (mut manager, registry, client) = make_manager();
        manager
            .create("agent-1", &spec(), AgentRegistration::default())
            .await
            .unwrap();
        let agent = manager.live.get_mut("agent-1").unwrap();
        agent.session_use = SessionUse::Configuration(AgentRuntimeStatus::Idle);
        let mut info = agent.record.runtime_info.clone().unwrap();
        info.model = Some("updated-model".into());
        if queued_event {
            client
                .0
                .lock()
                .unwrap()
                .events
                .push_back(AgentTurnEvent::RuntimeInfo(info.clone()));
        } else {
            agent.pending_runtime = Some(info.clone());
        }
        registry.0.lock().unwrap().fail_update = true;

        assert_eq!(
            manager.release_configuration_session("agent-1").await,
            Err(AgentManagerError::Registry)
        );
        assert!(!manager.live.contains_key("agent-1"));
        assert!(
            manager
                .pending_configuration_cleanup
                .contains_key("agent-1")
        );
        assert_eq!(manager.session_budget.available_permits(), 32);

        registry.0.lock().unwrap().fail_update = false;
        manager
            .release_configuration_session("agent-1")
            .await
            .unwrap();
        assert_eq!(
            registry.get("agent-1").unwrap().unwrap().runtime_info,
            Some(info)
        );
        assert!(manager.pending_configuration_cleanup.is_empty());
    }
}
