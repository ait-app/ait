use super::*;
use std::time::Duration;

#[tokio::test(start_paused = true)]
async fn cold_start_has_a_separate_budget_from_question_acceptance() {
    let (mut f, specs) = future_fixture(Some("selected-model"));
    f.runner.0.lock().expect("runner").prepare_delay = Duration::from_secs(15);
    let before = f.registry.get("agent-1").expect("source record");

    let response = f
        .manager
        .side_ask(ask("cold-start"))
        .await
        .expect("cold startup must not expire at the question's admission deadline");

    assert_eq!(response.receipt, SideReceiptState::Accepted);
    assert_eq!(specs.lock().expect("preparations").len(), 1);
    assert_eq!(f.runner.0.lock().expect("runner").asks, 1);
    assert_eq!(f.main.0.lock().expect("main").create_calls, 0);
    assert_eq!(f.main.0.lock().expect("main").start_calls, 0);
    assert_eq!(f.registry.get("agent-1").expect("source record"), before);
}

#[tokio::test(start_paused = true)]
async fn failed_or_expired_preparation_returns_its_rejection_and_allows_a_fresh_request() {
    for delay in [Duration::ZERO, Duration::from_secs(60)] {
        let (mut f, specs) = future_fixture(Some("selected-model"));
        {
            let mut state = f.runner.0.lock().expect("runner");
            state.prepare_delay = delay;
            state.prepare_error = Some(AgentSessionError::Failed);
        }
        let before = f.registry.get("agent-1").expect("source record");
        let started = tokio::time::Instant::now();

        let rejected = f
            .manager
            .side_ask(ask("failed-start"))
            .await
            .expect("no question was sent, so acceptance is known to be rejected");

        assert_eq!(rejected.receipt, SideReceiptState::Rejected);
        assert_eq!(
            rejected.reason.as_deref(),
            Some("runtime_inspection_failed")
        );
        assert!(started.elapsed() < Duration::from_secs(60));
        assert_eq!(f.runner.0.lock().expect("runner").asks, 0);
        assert!(!f.manager.side_runners.contains_key("agent-1"));
        assert!(!f.manager.side_permits.contains_key("agent-1"));
        assert!(
            !f.manager
                .side_store
                .as_ref()
                .expect("side store")
                .uncertain_active("agent-1", "")
                .expect("no unresolved acceptance")
        );
        let replay = f
            .manager
            .side_ask(ask("failed-start"))
            .await
            .expect("replay the persisted rejection");
        assert_eq!(replay.receipt, rejected.receipt);
        assert_eq!(replay.reason, rejected.reason);
        assert_eq!(replay.record_id, rejected.record_id);
        assert_eq!(specs.lock().expect("preparations").len(), 1);

        {
            let mut state = f.runner.0.lock().expect("runner");
            state.prepare_delay = Duration::ZERO;
            state.prepare_error = None;
        }
        assert_eq!(
            f.manager
                .side_ask(ask("fresh-start"))
                .await
                .expect("fresh request after the runtime recovers")
                .receipt,
            SideReceiptState::Accepted
        );
        assert_eq!(specs.lock().expect("preparations").len(), 2);
        assert_eq!(f.runner.0.lock().expect("runner").asks, 1);
        assert_eq!(f.main.0.lock().expect("main").start_calls, 0);
        assert_eq!(f.registry.get("agent-1").expect("source record"), before);
    }
}
