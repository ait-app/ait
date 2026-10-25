use super::*;

#[test]
fn forwards_only_children_announced_by_the_owned_native_process() {
    let mut children = Children::default();
    let images = ImageStore::default();
    let message = json!({"type":"subagent_event","payload":{"id":"child","event":{"type":"message_end","message":{"role":"assistant","timestamp":1000,"content":[{"type":"text","text":"Hello"}]}}}});
    assert_eq!(
        children.consume(&message, "root", "/tmp", &images).unwrap(),
        Vec::new()
    );
    let lifecycle = json!({"type":"subagent_lifecycle","payload":{"id":"child","agent":"Worker","status":"started","parentToolCallId":"call"}});
    assert_eq!(
        children
            .consume(&lifecycle, "root", "/tmp", &images)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(children.list()[0].parent_id, "root");
    let events = children.consume(&message, "root", "/tmp", &images).unwrap();
    assert!(
        matches!(&events[0],AgentTurnEvent::Subagent(SubagentEvent::Timeline{id,entry}) if id=="child" && entry.item["text"]=="Hello")
    );
    let progress = json!({"type":"subagent_progress","payload":{"agent":"Worker","progress":{"id":"child","status":"completed","task":"Task","resolvedModel":"test/model"}}});
    children
        .consume(&progress, "root", "/tmp", &images)
        .unwrap();
    assert_eq!(children.list()[0].descriptor["status"], "completed");
    assert_eq!(children.list()[0].descriptor["subtitle"], "test/model");
    assert!(
        children
            .consume(&lifecycle, "child", "/tmp", &images)
            .is_err()
    );
}

#[test]
fn child_images_use_the_native_blob_store() {
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    let hash = format!("{:x}", Sha256::digest(b"child-image"));
    std::fs::write(root.path().join(&hash), b"child-image").unwrap();
    let mut children = Children::default().with_blobs(root.path().to_path_buf());
    let images = ImageStore::new(root.path().join("images"));
    children
        .consume(
            &json!({"type":"subagent_lifecycle","payload":{"id":"child","status":"started"}}),
            "parent",
            "/work",
            &images,
        )
        .unwrap();
    let events = children.consume(&json!({"type":"subagent_event","payload":{"id":"child","event":{"type":"message_end","message":{"role":"assistant","timestamp":1000,"content":[{"type":"image","mimeType":"image/png","data":format!("blob:sha256:{hash}")}]}}}}), "parent", "/work", &images).unwrap();
    assert!(events.iter().any(|event| matches!(event, AgentTurnEvent::Subagent(SubagentEvent::Timeline { entry, .. }) if entry.item["text"].as_str().unwrap().contains("![Image]"))));
}
