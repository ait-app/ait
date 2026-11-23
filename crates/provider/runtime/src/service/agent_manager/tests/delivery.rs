use std::time::Duration;

use provider_core::ports::agent_session::CommandRoute;
use provider_core::protocol::prompt::AgentPrompt;

use super::*;
use crate::protocol::agent_execution::ActiveTurnBehavior;
use crate::storage::timeline::Timeline;
use crate::storage::timeline::inbox::Receipt;

#[tokio::test]
async fn command_ownership_controls_admission_without_interrupting_an_unknown_turn() {
    let (manager, _, client) = make_manager();
    let timeline = Timeline::memory().expect("timeline");
    let mut manager = manager.with_timeline(timeline);
    manager
        .create("agent-1", &spec(), AgentRegistration::default())
        .await
        .unwrap();
    manager.send("agent-1", "running").await.unwrap();
    {
        let mut state = client.0.lock().unwrap();
        state.command_ownership = CommandOwnership::Supported;
        state.command_route = Some(CommandRoute::Unavailable);
    }
    let prompt = AgentPrompt {
        client_message_id: Some("slash-1".into()),
        ..AgentPrompt::text("/alias")
    };
    assert_eq!(
        manager
            .deliver("agent-1", &prompt, ActiveTurnBehavior::Interrupt)
            .await,
        Err(ErrorCode::UnsupportedCapability)
    );
    assert!(manager.active_turn("agent-1").is_some());
    assert_eq!(client.0.lock().unwrap().out_of_band_calls, 0);
    client.0.lock().unwrap().command_route = Some(CommandRoute::OutOfBand);
    manager
        .deliver("agent-1", &prompt, ActiveTurnBehavior::Interrupt)
        .await
        .unwrap();
    assert!(manager.active_turn("agent-1").is_some());
    client.0.lock().unwrap().command_route = Some(CommandRoute::Foreground);
    manager
        .deliver("agent-1", &prompt, ActiveTurnBehavior::Interrupt)
        .await
        .unwrap();
    assert_eq!(client.0.lock().unwrap().out_of_band_calls, 1);
    assert!(manager.active_turn("agent-1").is_some());
}

#[tokio::test]
async fn native_claimant_is_delivered_as_foreground_input() {
    let (manager, _, client) = make_manager();
    let mut manager = manager.with_timeline(Timeline::memory().unwrap());
    manager
        .create("agent-1", &spec(), AgentRegistration::default())
        .await
        .unwrap();
    {
        let mut state = client.0.lock().unwrap();
        state.command_ownership = CommandOwnership::Supported;
        state.command_route = Some(CommandRoute::Foreground);
    }
    let prompt = AgentPrompt {
        client_message_id: Some("native-command".into()),
        ..AgentPrompt::text("/model")
    };
    manager
        .deliver("agent-1", &prompt, ActiveTurnBehavior::Interrupt)
        .await
        .unwrap();
    assert_eq!(client.0.lock().unwrap().out_of_band_calls, 0);
    assert_eq!(client.0.lock().unwrap().start_calls, 1);
    client.0.lock().unwrap().command_route = Some(CommandRoute::OutOfBand);
    manager
        .deliver("agent-1", &prompt, ActiveTurnBehavior::Interrupt)
        .await
        .unwrap();
    assert_eq!(client.0.lock().unwrap().start_calls, 1);
}

#[tokio::test]
async fn slash_prefixed_paths_remain_foreground_text() {
    for text in ["/tmp/file please inspect", "/"] {
        let (manager, _, client) = make_manager();
        let mut manager = manager.with_timeline(Timeline::memory().unwrap());
        manager
            .create("agent-1", &spec(), AgentRegistration::default())
            .await
            .unwrap();
        client.0.lock().unwrap().command_ownership = CommandOwnership::Supported;
        let prompt = AgentPrompt::text(text);
        manager
            .deliver("agent-1", &prompt, ActiveTurnBehavior::Interrupt)
            .await
            .unwrap();
        assert_eq!(client.0.lock().unwrap().out_of_band_calls, 0);
        assert_eq!(client.0.lock().unwrap().start_calls, 1);
    }
}

#[tokio::test]
async fn command_catalog_updates_publish_a_session_scoped_event() {
    let (manager, _, client) = make_manager();
    let timeline = Timeline::memory().unwrap();
    let (outbound, mut received) = model::outbound::Outbound::new();
    let subscription = timeline.events().subscribe(
        "observer".to_owned(),
        std::collections::BTreeSet::from(["agent-1".to_owned()]),
        outbound,
    );
    subscription.activate().unwrap();
    let mut manager = manager.with_timeline(timeline);
    manager
        .create("agent-1", &spec(), AgentRegistration::default())
        .await
        .unwrap();
    client
        .0
        .lock()
        .unwrap()
        .events
        .push_back(AgentTurnEvent::CommandsChanged);
    manager.poll().await.unwrap();
    let frame = received.try_recv().expect("catalog change event");
    let model::outbound::Frame::Text(text) = frame.message else {
        panic!("JSON event")
    };
    let event: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(event["method"], "agent_stream");
    assert_eq!(event["params"]["agentId"], "agent-1");
    assert_eq!(event["params"]["event"]["type"], "commands_changed");
}

#[tokio::test]
async fn command_catalog_change_does_not_leave_a_fake_running_turn() {
    let (manager, registry, client) = make_manager();
    let timeline = Timeline::memory().unwrap();
    let mut manager = manager.with_timeline(timeline.clone());
    manager
        .create("agent-1", &spec(), AgentRegistration::default())
        .await
        .unwrap();
    registry
        .update("agent-1", &|current| {
            let mut next = current.clone();
            next.requires_attention = true;
            next.attention_reason = Some(domain::agent_runtime::AgentAttentionReason::Finished);
            next.attention_timestamp = Some("2026-01-01T00:00:00Z".into());
            next
        })
        .unwrap();
    let before = registry.get("agent-1").unwrap().unwrap();
    {
        let mut state = client.0.lock().unwrap();
        state.command_ownership = CommandOwnership::Supported;
        state.command_route = Some(CommandRoute::Foreground);
        state.start_result = Err(AgentSessionError::CommandCatalogChanged);
    }
    let prompt = AgentPrompt {
        client_message_id: Some("restart-command".into()),
        ..AgentPrompt::text("/model")
    };
    assert_eq!(
        manager
            .deliver("agent-1", &prompt, ActiveTurnBehavior::Interrupt)
            .await,
        Err(ErrorCode::CatalogBusy)
    );
    let after = registry.get("agent-1").unwrap().unwrap();
    assert_eq!(after.last_status, before.last_status);
    assert_eq!(after.requires_attention, before.requires_attention);
    assert_eq!(after.attention_reason, before.attention_reason);
    assert_eq!(after.attention_timestamp, before.attention_timestamp);
    assert_eq!(after.last_user_message_at, before.last_user_message_at);
    assert_eq!(after.last_activity_at, before.last_activity_at);
    assert_eq!(after.title, before.title);
    assert!(manager.active_turn("agent-1").is_none());
    assert_eq!(
        timeline
            .input_receipt("agent-1", "restart-command", &prompt, "interrupt")
            .unwrap(),
        Some(Receipt::Rejected)
    );
}

#[tokio::test]
async fn queued_command_is_not_sent_after_its_ownership_changes() {
    let (manager, _, client) = make_manager();
    let timeline = Timeline::memory().unwrap();
    let mut manager = manager.with_timeline(timeline.clone());
    manager
        .create("agent-1", &spec(), AgentRegistration::default())
        .await
        .unwrap();
    {
        let mut state = client.0.lock().unwrap();
        state.command_ownership = CommandOwnership::Supported;
        state.command_route = Some(CommandRoute::OutOfBand);
    }
    let prompt = AgentPrompt {
        client_message_id: Some("queued-command".into()),
        ..AgentPrompt::text("/model")
    };
    timeline
        .reserve_input("agent-1", "queued-command", &prompt, "interrupt")
        .unwrap();
    manager.dispatch_pending_inputs().await.unwrap();
    assert_eq!(client.0.lock().unwrap().start_calls, 0);
    assert_eq!(client.0.lock().unwrap().out_of_band_calls, 0);
    assert_eq!(
        timeline
            .input_receipt("agent-1", "queued-command", &prompt, "interrupt")
            .unwrap(),
        Some(Receipt::Rejected)
    );
}

#[tokio::test]
async fn queued_command_waits_for_the_session_catalog_without_losing_its_receipt() {
    let (manager, _, client) = make_manager();
    let timeline = Timeline::memory().unwrap();
    let mut manager = manager.with_timeline(timeline.clone());
    manager
        .create("agent-1", &spec(), AgentRegistration::default())
        .await
        .unwrap();
    {
        let mut state = client.0.lock().unwrap();
        state.command_ownership = CommandOwnership::Supported;
        state.command_route = Some(CommandRoute::Pending);
    }
    let prompt = AgentPrompt {
        client_message_id: Some("waiting-command".into()),
        ..AgentPrompt::text("/model")
    };
    timeline
        .reserve_input("agent-1", "waiting-command", &prompt, "interrupt")
        .unwrap();
    manager.dispatch_pending_inputs().await.unwrap();
    assert_eq!(
        timeline
            .input_delivery_state("agent-1", "waiting-command")
            .unwrap()
            .as_deref(),
        Some("queued")
    );
    assert_eq!(client.0.lock().unwrap().start_calls, 0);
    client.0.lock().unwrap().command_route = Some(CommandRoute::Foreground);
    manager.dispatch_pending_inputs().await.unwrap();
    assert_eq!(
        timeline
            .input_receipt("agent-1", "waiting-command", &prompt, "interrupt")
            .unwrap(),
        Some(Receipt::Accepted)
    );
    assert_eq!(client.0.lock().unwrap().start_calls, 1);
}

#[tokio::test]
async fn refused_managed_command_has_a_stable_rejection_receipt() {
    let (manager, _, client) = make_manager();
    let timeline = Timeline::memory().unwrap();
    let mut manager = manager.with_timeline(timeline.clone());
    manager
        .create("agent-1", &spec(), AgentRegistration::default())
        .await
        .unwrap();
    {
        let mut state = client.0.lock().unwrap();
        state.command_ownership = CommandOwnership::Supported;
        state.command_route = Some(CommandRoute::OutOfBand);
        state.out_of_band_result = Err(AgentSessionError::Rejected);
    }
    let prompt = AgentPrompt {
        client_message_id: Some("refused-command".into()),
        ..AgentPrompt::text("/model")
    };
    for _ in 0..2 {
        assert_eq!(
            manager
                .deliver("agent-1", &prompt, ActiveTurnBehavior::Interrupt)
                .await,
            Err(ErrorCode::InvalidMessage)
        );
    }
    assert_eq!(client.0.lock().unwrap().out_of_band_calls, 1);
    assert_eq!(
        timeline
            .input_receipt("agent-1", "refused-command", &prompt, "answer")
            .unwrap(),
        Some(Receipt::Rejected)
    );
}

#[tokio::test(start_paused = true)]
async fn stalled_pending_cancellation_closes_session_and_dispatches_input() {
    assert_dispatch_after_cancellation_timeout(CancellationStall::Pending).await;
}

#[tokio::test(start_paused = true)]
async fn stalled_turn_cancellation_closes_session_and_dispatches_input() {
    assert_dispatch_after_cancellation_timeout(CancellationStall::Turn).await;
}

async fn assert_dispatch_after_cancellation_timeout(stall: CancellationStall) {
    const AGENT: &str = "agent-1";
    const MESSAGE: &str = "follow-up";

    let (manager, _, client) = make_manager();
    let timeline = Timeline::memory().expect("timeline");
    let mut manager = manager.with_timeline(timeline.clone());
    manager
        .create(AGENT, &spec(), AgentRegistration::default())
        .await
        .expect("create agent");
    manager
        .send(AGENT, "first prompt")
        .await
        .expect("start turn");
    assert!(manager.active_turn(AGENT).is_some());
    client.0.lock().expect("state").cancellation_stall = Some(stall);
    let prompt = AgentPrompt {
        client_message_id: Some(MESSAGE.to_owned()),
        ..AgentPrompt::text("next prompt")
    };

    tokio::time::timeout(
        Duration::from_secs(11),
        manager.deliver(AGENT, &prompt, ActiveTurnBehavior::Interrupt),
    )
    .await
    .expect("cancellation must release delivery within its budget")
    .expect("dispatch queued input after closing stalled session");

    assert!(manager.active_turn(AGENT).is_some());
    assert!(!timeline.has_queued_input(AGENT).expect("inspect queue"));
    assert_eq!(
        timeline
            .input_receipt(AGENT, MESSAGE, &prompt, "interrupt")
            .expect("read receipt"),
        Some(Receipt::Accepted)
    );
    manager
        .deliver(AGENT, &prompt, ActiveTurnBehavior::Interrupt)
        .await
        .expect("retry accepted input");
    let state = client.0.lock().expect("state");
    assert_eq!(state.close_calls, 1);
    assert_eq!(state.resume_purposes, [AgentResumePurpose::Interactive]);
    assert_eq!(state.start_calls, 2);
}

#[tokio::test]
async fn preset_save_after_claim_requeues_without_native_write() {
    use persistence::storage::daemon_config::FileDaemonConfigStore;
    use provider_core::ports::preset::Preset;
    let root = tempfile::tempdir().unwrap();
    let service = crate::service::presets::PresetService::new(Arc::new(
        FileDaemonConfigStore::with_defaults(root.path().join("config.json")),
    ));
    let make_preset = |model: &str| Preset {
        name: "team".into(),
        model: model.into(),
        thinking_option_id: None,
        feature_values: BTreeMap::new(),
    };
    let cwd = spec().cwd;
    service
        .save("codex", &cwd, None, &make_preset("first-model"), None)
        .await
        .unwrap();
    let (manager, _, client) = make_manager();
    let timeline = Timeline::memory().unwrap();
    let mut manager = manager
        .with_timeline(timeline.clone())
        .with_presets(service.clone());
    manager
        .create("agent-1", &spec(), AgentRegistration::default())
        .await
        .unwrap();
    let prepared = manager
        .prepare_preset("agent-1", "team", "ait", None)
        .unwrap()
        .await
        .unwrap()
        .unwrap();
    manager.commit_preset(&prepared).unwrap();
    service.wait_lookup_finished().await;
    assert!(!manager.ensure_preset_ready("agent-1").unwrap());
    tokio::time::timeout(Duration::from_secs(3), service.wait_lookup_finished())
        .await
        .unwrap();
    assert!(manager.ensure_preset_ready("agent-1").unwrap());
    let prompt = AgentPrompt {
        client_message_id: Some("initial".into()),
        ..AgentPrompt::text("first")
    };
    timeline
        .reserve_input("agent-1", "initial", &prompt, "interrupt")
        .unwrap();
    timeline.claim_input("agent-1", "initial").unwrap();
    service
        .save("codex", &cwd, None, &make_preset("updated-model"), None)
        .await
        .unwrap();
    assert_eq!(
        manager.send_input("agent-1", &prompt).await,
        Err(AgentManagerError::PresetPending)
    );
    assert_eq!(client.0.lock().unwrap().start_calls, 0);
    timeline.requeue_input("agent-1", "initial").unwrap();
    tokio::time::timeout(Duration::from_secs(3), service.wait_lookup_finished())
        .await
        .unwrap();
    manager.dispatch_pending_inputs().await.unwrap();
    assert_eq!(
        timeline
            .input_delivery_state("agent-1", "initial")
            .unwrap()
            .as_deref(),
        Some("accepted")
    );
    let fake = client.0.lock().unwrap();
    assert_eq!(fake.start_calls, 1);
    assert_eq!(
        fake.start_configs[0].model.as_deref(),
        Some("updated-model")
    );
}
