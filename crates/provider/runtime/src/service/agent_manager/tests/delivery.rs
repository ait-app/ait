use std::time::Duration;

use provider_core::protocol::prompt::AgentPrompt;

use super::*;
use crate::protocol::agent_execution::ActiveTurnBehavior;
use crate::storage::timeline::Timeline;
use crate::storage::timeline::inbox::Receipt;

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
