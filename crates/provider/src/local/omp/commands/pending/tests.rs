use super::*;
use serde_json::json;

#[tokio::test]
async fn official_builtin_completion_is_explicitly_final_without_a_prompt_result() {
    let mut pending = Pending::default();
    pending.query(
        "query".to_owned(),
        "m".to_owned(),
        "usage",
        Duration::from_secs(1),
    );
    let receipt = pending.take().unwrap();
    pending.capture(&json!({"type":"command_output","text":"Native usage"}));
    let Capture::Finished(entry)=pending.capture(&json!({"type":"response","id":"query","command":"prompt","success":true,"data":{"agentInvoked":false}})) else { panic!("builtin final response was not recognized") };
    receipt.await.unwrap();
    assert_eq!(entry.item["text"], "Native usage");
    assert!(pending.status().is_none());
}

#[tokio::test]
async fn query_requires_its_final_result_and_does_not_consume_foreground_frames() {
    let mut pending = Pending::default();
    pending.query(
        "query".to_owned(),
        "message".to_owned(),
        "usage",
        Duration::from_secs(1),
    );
    let receipt = pending.take().unwrap();
    assert!(matches!(
        pending.capture(&json!({"type":"response","id":"query","command":"prompt","success":true})),
        Capture::Pending
    ));
    assert!(matches!(
        pending.capture(&json!({"type":"message_update","id":"foreground"})),
        Capture::Unrelated
    ));
    assert!(pending.check(true).is_err());
    pending.capture(&json!({"type":"command_output","text":"Usage: 12"}));
    let Capture::Finished(entry) = pending.capture(
        &json!({"type":"prompt_result","id":"query","status":"completed","agentInvoked":false}),
    ) else {
        panic!("expected native query result")
    };
    receipt.await.unwrap();
    assert_eq!(entry.item["type"], "provider_command");
    assert_eq!(entry.item["text"], "Usage: 12");
    assert!(entry.turn_id.is_none());
    assert!(pending.check(true).is_ok());
}

#[tokio::test]
async fn rejection_and_close_resolve_detached_receipts_without_foreground_completion() {
    let mut pending = Pending::default();
    pending.rpc("side".to_owned(), "btw", Duration::from_secs(1));
    let receipt = pending.take().unwrap();
    pending.capture(
        &json!({"type":"response","id":"side","command":"btw","success":false,"error":"no model"}),
    );
    assert_eq!(receipt.await, Err(AgentSessionError::Rejected));
    pending.rpc(
        "history".to_owned(),
        "get_btw_history",
        Duration::from_secs(1),
    );
    let receipt = pending.take().unwrap();
    pending.close();
    assert_eq!(receipt.await, Err(AgentSessionError::Failed));
}

#[tokio::test]
async fn expired_queries_retain_output_correlation_until_native_settlement() {
    let mut pending = Pending::default();
    pending.query("slow".to_owned(), "m".to_owned(), "usage", Duration::ZERO);
    let receipt = pending.take().unwrap();
    assert_eq!(pending.expired().unwrap().item["status"], "error");
    assert!(receipt.await.is_err());
    assert!(pending.expired().is_none());
    assert!(pending.check(true).is_err());
    assert!(matches!(
        pending.capture(&json!({"type":"command_output","text":"late output"})),
        Capture::Pending
    ));
    assert!(matches!(
        pending.capture(&json!({"type":"prompt_result","id":"slow","status":"completed"})),
        Capture::Finished(_)
    ));
    assert!(pending.check(true).is_ok());
}

#[test]
fn oversized_unicode_output_is_clipped_without_splitting_text() {
    let mut pending = Pending::default();
    pending.query(
        "q".to_owned(),
        "m".to_owned(),
        "context",
        Duration::from_secs(1),
    );
    for _ in 0..3 {
        pending.capture(&json!({"type":"command_output","text":"界".repeat(30000)}));
    }
    let Capture::Finished(entry) =
        pending.capture(&json!({"type":"prompt_result","id":"q","status":"completed"}))
    else {
        panic!("missing result")
    };
    assert!(entry.item["text"].as_str().unwrap().len() <= 64 * 1024);
}
