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

#[test]
fn preview_wire_omits_absent_turn_without_changing_native_storage() {
    let entries = [
        super::NativeItem {
            key: "user-0".into(),
            turn_id: None,
            timestamp: "2026-01-01T00:00:00Z".into(),
            item: json!({"type":"user_message","text":"question"}),
        },
        super::NativeItem {
            key: "assistant-0".into(),
            turn_id: Some("native-turn-0".into()),
            timestamp: "2026-01-01T00:00:01Z".into(),
            item: json!({"type":"assistant_message","text":"answer"}),
        },
    ];
    let wire = serde_json::to_value(super::PreviewEntries(&entries))
        .expect("preview entries should serialize");
    assert!(wire[0].get("turnId").is_none());
    assert_eq!(wire[1]["turnId"], "native-turn-0");
    assert_eq!(wire[0]["item"], entries[0].item);
    assert_eq!(wire[1]["key"], entries[1].key);
    let stored = serde_json::to_value(&entries[0]).expect("native item should serialize");
    assert_eq!(stored.get("turnId"), Some(&serde_json::Value::Null));
    let restored: super::NativeItem =
        serde_json::from_value(stored).expect("persisted absent turn should remain readable");
    assert_eq!(restored, entries[0]);
}
