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
