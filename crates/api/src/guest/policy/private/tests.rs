use std::future::ready;

use super::*;

#[tokio::test]
async fn agents_created_after_revocation_are_cancelled_in_both_response_shapes() {
    for reply in [json!({"agentId": "late"}), json!({"agent": {"id": "late"}})] {
        let mut cancelled = None;
        let result = finish_agent(&reply, false, |id| {
            cancelled = Some(id);
            ready(Ok(()))
        })
        .await;
        assert_eq!(result, Err(ErrorCode::Forbidden));
        assert_eq!(cancelled.as_deref(), Some("late"));
    }
    let mut called = false;
    assert_eq!(
        finish_agent(&json!({"agentId": "active"}), true, |_| {
            called = true;
            ready(Ok(()))
        })
        .await,
        Ok(())
    );
    assert!(!called);
}

#[tokio::test]
async fn failed_creation_after_revocation_does_not_cancel_an_unrelated_agent() {
    let mut called = false;
    assert_eq!(
        finish_agent(&json!({"agent": null}), false, |_| {
            called = true;
            ready(Ok(()))
        })
        .await,
        Err(ErrorCode::Forbidden)
    );
    assert!(!called);
}
