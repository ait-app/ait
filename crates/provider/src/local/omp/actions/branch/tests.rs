use super::*;

#[tokio::test]
async fn branch_requires_matching_acknowledgement_final_receipt_and_host_commit() {
    let mut pending = Pending::default();
    let mut future = pending.btw_branch("request".to_owned(), "nonce".to_owned());
    assert!(pending.changing_history());
    assert!(
        !pending
            .capture(&json!({"type":"response","id":"foreign","command":"prompt","success":true}))
    );
    assert!(
        pending
            .capture(&json!({"type":"response","id":"request","command":"prompt","success":true}))
    );
    assert!(pending.take_refresh().is_none());
    assert!(pending.capture(
        &json!({"type":"extension_ui_request","method":"notify","message":
        "AIT_BTW_BRANCH_V1:{\"requestId\":\"foreign\",\"success\":true}"})
    ));
    assert!(pending.take_refresh().is_none());
    assert!(pending.capture(
        &json!({"type":"extension_ui_request","method":"notify","message":
        "AIT_BTW_BRANCH_V1:{\"requestId\":\"nonce\",\"success\":true}"})
    ));
    assert!(pending.take_refresh().is_none());
    assert!(pending.capture(
        &json!({"type":"prompt_result","id":"request","status":"completed","agentInvoked":false})
    ));
    let refresh = pending.take_refresh().unwrap();
    assert_eq!(refresh.action, "btw_branch");
    pending.prepared(refresh);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(1), &mut future)
            .await
            .is_err()
    );
    pending.committed();
    assert!(future.await.unwrap()["error"].is_null());
    assert!(!pending.changing_history());
}

#[tokio::test]
async fn branch_failure_still_reconciles_the_native_owner_before_reporting_error() {
    let mut pending = Pending::default();
    let future = pending.btw_branch("request".to_owned(), "nonce".to_owned());
    assert!(pending.capture(
        &json!({"type":"prompt_result","id":"request","status":"completed","agentInvoked":false})
    ));
    let refresh = pending.take_refresh().unwrap();
    pending.prepared(refresh);
    pending.committed();
    assert!(
        future.await.unwrap()["error"]
            .as_str()
            .unwrap()
            .contains("acknowledge")
    );
}
