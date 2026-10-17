use serde_json::json;

use super::ContextCompactRequest;

#[test]
fn destination_compaction_request_keeps_exact_provider_selection() {
    let request: ContextCompactRequest = serde_json::from_value(json!({
        "cwd":"/repo",
        "provider":"claude",
        "model":"sonnet",
        "thinkingOptionId":"high",
        "context":"older conversation"
    }))
    .expect("compaction request should decode");

    assert_eq!(request.provider, "claude");
    assert_eq!(request.model.as_deref(), Some("sonnet"));
    assert_eq!(request.thinking_option_id.as_deref(), Some("high"));
}

#[test]
fn destination_compaction_request_rejects_unknown_fields() {
    assert!(
        serde_json::from_value::<ContextCompactRequest>(json!({
            "cwd":"/repo",
            "provider":"claude",
            "model":null,
            "thinkingOptionId":null,
            "context":"older conversation",
            "agentId":"old-agent"
        }))
        .is_err()
    );
}
