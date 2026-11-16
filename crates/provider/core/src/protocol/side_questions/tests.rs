use super::*;

#[test]
fn native_followup_drives_status_without_discarding_root_or_images() {
    let record: BtwHistoryRecord = serde_json::from_value(serde_json::json!({
        "id": "native-topic", "leafId": null, "question": "first", "answer": "done",
        "status": "complete", "createdAt": 10, "updatedAt": 20,
        "images": [{"type": "image", "mimeType": "image/png", "data": "AA=="}],
        "followUps": [{"question": "second", "answer": "partial", "status": "running",
            "createdAt": 30, "updatedAt": 40}],
        "futureNativeField": true
    }))
    .unwrap();
    assert_eq!(record.latest_turn().status, SideTurnStatus::Running);
    assert_eq!(record.latest_turn().question, "second");
    assert_eq!(record.status, SideTurnStatus::Complete);
    assert_eq!(record.images[0].mime_type, "image/png");
    let wire = serde_json::to_value(record).unwrap();
    assert_eq!(wire["images"][0]["data"], "AA==");
    assert!(wire["leafId"].is_null());
}

#[test]
fn restarted_native_turn_is_terminal_without_becoming_complete() {
    let record: BtwHistoryRecord = serde_json::from_value(serde_json::json!({
        "id": "native-topic", "leafId": "leaf", "question": "first", "answer": "partial",
        "status": "interrupted", "createdAt": 10, "updatedAt": 20
    }))
    .unwrap();
    assert!(record.latest_turn().status.is_terminal());
    assert_ne!(record.latest_turn().status, SideTurnStatus::Complete);
    assert_eq!(record.latest_turn().answer, "partial");
}

#[test]
fn byte_bound_never_splits_multibyte_content() {
    let text = "héllo世界";
    assert_eq!(bounded(text, 2), ("h", true));
    assert_eq!(bounded(text, 4), ("hél", true));
    assert_eq!(bounded(text, text.len()), (text, false));
}
