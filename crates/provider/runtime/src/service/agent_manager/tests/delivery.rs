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
