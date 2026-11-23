use super::*;

const ID: &str = "dbcb4cff-d941-4467-968f-b08008369250";

fn start(agent: &str, id: &str) -> Incoming {
    Incoming::Text(
        ClientMessage::Request {
            request_id: "request".to_owned(),
            method: "agent.native_html.export.request".to_owned(),
            params: serde_json::json!({"agentId":agent,"operationId":id}),
        },
        None,
    )
}

fn cancel(agent: &str, id: &str) -> Value {
    serde_json::json!({"agentId":agent,"operationId":id})
}

#[test]
fn cancellation_before_admission_prevents_the_late_request() {
    let ledger = NativeExports::default();
    assert!(ledger.cancel(&cancel("agent", ID)).unwrap());
    assert!(matches!(
        ledger.admit(&mut start("agent", ID)),
        Err(ErrorCode::AgentIo)
    ));
    assert!(!ledger.cancel(&cancel("other", ID)).unwrap());
}

#[test]
fn cancellation_and_completion_are_scoped_and_replays_expire() {
    let ledger = NativeExports::default();
    let lease = ledger.admit(&mut start("agent", ID)).unwrap();
    assert!(matches!(
        ledger.admit(&mut start("agent", ID)),
        Err(ErrorCode::IdempotencyConflict)
    ));
    assert!(!ledger.cancel(&cancel("other", ID)).unwrap());
    assert!(ledger.cancel(&cancel("agent", ID)).unwrap());
    assert!(lease.cancel.is_cancelled());
    drop(lease);
    assert!(ledger.cancel(&cancel("agent", ID)).unwrap());
    ledger.0.lock().unwrap().get_mut(ID).unwrap().status =
        Status::Cancelled(Instant::now() - RECEIPT_TTL);
    let finished = ledger.admit(&mut start("agent", ID)).unwrap();
    drop(finished);
    assert!(!ledger.cancel(&cancel("agent", ID)).unwrap());
    assert!(matches!(
        ledger.admit(&mut start("agent", ID)),
        Err(ErrorCode::IdempotencyConflict)
    ));
}

#[test]
fn ledger_bounds_and_validates_untrusted_cancel_requests() {
    let ledger = NativeExports::default();
    for _ in 0..MAX_RECEIPTS {
        let id = uuid::Uuid::new_v4().to_string();
        assert!(ledger.cancel(&cancel("agent", &id)).unwrap());
    }
    assert_eq!(
        ledger.cancel(&cancel("agent", ID)),
        Err(ErrorCode::ResourceExhausted)
    );
    assert_eq!(
        ledger.cancel(&serde_json::json!({"agentId":"agent", "operationId": ID, "token":"x"})),
        Err(ErrorCode::InvalidMessage)
    );
    assert_eq!(
        ledger.cancel(&cancel("agent", "not-a-uuid")),
        Err(ErrorCode::InvalidMessage)
    );
}
