use super::*;

#[derive(Debug, Default)]
struct State {
    calls: usize,
    cancelled: bool,
}

#[derive(Debug)]
struct Pending(Arc<Mutex<State>>);

#[async_trait::async_trait]
impl SideQuestions for Pending {
    fn side_strategy(&self) -> SideStrategy {
        SideStrategy::Isolated
    }
    fn side_capability(&self) -> SideCapability {
        SideCapability::available()
    }
    async fn side_ask(&mut self, _: SideAsk) -> Result<SideAskReceipt, AgentSessionError> {
        self.0.lock().expect("pending fixture").calls += 1;
        std::future::pending().await
    }
    async fn side_cancel(
        &mut self,
        _: Option<&str>,
    ) -> Result<SideCancelOutcome, AgentSessionError> {
        self.0.lock().expect("pending fixture").cancelled = true;
        Ok(SideCancelOutcome { cancelled: true })
    }
    async fn side_history(&mut self) -> Result<Vec<SideTopic>, AgentSessionError> {
        Ok(Vec::new())
    }
    async fn reconcile_side(&mut self, _: &str) -> Result<Option<SideTopic>, AgentSessionError> {
        if self.0.lock().expect("pending fixture").cancelled {
            Ok(None)
        } else {
            Err(AgentSessionError::Unavailable)
        }
    }
    fn side_projection(&self) -> SideQuestionsState {
        SideQuestionsState::default()
    }
}

#[tokio::test(start_paused = true)]
async fn admission_deadline_is_uncertain_and_only_confirmed_cancellation_unlocks_new_work() {
    let mut f = fixture();
    let state = Arc::new(Mutex::new(State::default()));
    f.manager
        .side_runners
        .insert("agent-1".to_owned(), Box::new(Pending(state.clone())));
    let before = f.registry.get("agent-1").expect("main record");
    let response = f
        .manager
        .side_ask(ask("deadline"))
        .await
        .expect("bounded receipt");
    assert_eq!(response.receipt, SideReceiptState::Uncertain);
    assert_eq!(
        f.manager
            .side_projection("agent-1")
            .expect("uncertain state")
            .pending_acceptance,
        Some(true)
    );
    assert_eq!(
        f.manager
            .side_ask(ask("deadline"))
            .await
            .expect("immutable replay")
            .receipt,
        SideReceiptState::Uncertain
    );
    assert_eq!(state.lock().expect("pending fixture").calls, 1);
    assert!(
        f.manager
            .side_cancel(SideCancelRequest {
                agent_id: "agent-1".to_owned(),
                record_id: None
            })
            .await
            .expect("confirmed cancellation")
            .cancelled
    );
    assert_eq!(
        f.manager
            .side_projection("agent-1")
            .expect("reconciled state")
            .pending_acceptance,
        Some(false)
    );
    assert_eq!(
        f.manager
            .side_ask(ask("deadline"))
            .await
            .expect("settled replay")
            .receipt,
        SideReceiptState::Uncertain
    );
    assert_eq!(state.lock().expect("pending fixture").calls, 1);
    assert_eq!(f.main.0.lock().expect("main").start_calls, 0);
    assert_eq!(f.registry.get("agent-1").expect("main record"), before);
}
