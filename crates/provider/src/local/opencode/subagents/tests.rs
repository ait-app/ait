use super::*;
use crate::{
    local::images::ImageStore,
    ports::{agent_session::AgentTurnEvent, controls::SubagentEvent},
};

#[test]
fn legacy_task_completion_flushes_the_child_without_an_extension_status() {
    let mut live = live::Live::new("root".into(), "/work".into(), ImageStore::default());
    let message =
        |update| json!({"method":"session/update","params":{"sessionId":"root","update":update}});
    live.consume(&message(
        json!({"sessionUpdate":"tool_call","toolCallId":"task","title":"task",
        "status":"pending","rawInput":{"subagent_type":"explore","description":"Inspect"}}),
    ))
    .unwrap();
    live.consume(&message(
        json!({"sessionUpdate":"agent_message_chunk","messageId":"answer",
        "content":{"type":"text","text":"Legacy child"},"_meta":{"opencode/child-session":{
            "id":"child","parentID":"root","depth":1}}}),
    ))
    .unwrap();
    let (handled, events) = live.consume(&message(json!({"sessionUpdate":"tool_call_update","toolCallId":"task",
        "status":"completed","rawOutput":{"metadata":{"sessionId":"child","parentSessionId":"root"}}}))).unwrap();
    assert!(!handled);
    assert_eq!(live.list()[0].descriptor["status"], "completed");
    assert!(events.iter().any(|event| matches!(event, AgentTurnEvent::Subagent(SubagentEvent::Timeline { entry, .. }) if entry.item["text"] == "Legacy child")));
}

fn notification(id: &str, parent: &str, depth: u64, event: Value) -> Value {
    let mut params = json!({"rootSessionId":"root","childSessionId":id,"parentSessionId":parent,"depth":depth,"title":"Explore"});
    let Value::Object(event) = event else {
        panic!("expected fixture event object");
    };
    params.as_object_mut().unwrap().extend(event);
    json!({"method":"opencode/session/child_update","params":params})
}

#[test]
fn live_children_buffer_early_descendants_and_reject_conflicting_or_foreign_ownership() {
    let mut live = live::Live::new("root".into(), "/work".into(), ImageStore::default());
    let created = json!({"type":"status","status":"created"});
    assert!(
        live.consume(&notification("nested", "child", 2, created.clone()))
            .unwrap()
            .1
            .is_empty()
    );
    let (_, events) = live
        .consume(&notification("child", "root", 1, created.clone()))
        .unwrap();
    assert_eq!(live.list().len(), 2);
    assert_eq!(events.len(), 2);
    assert!(
        live.consume(&notification("child", "nested", 3, created.clone()))
            .is_err()
    );
    assert!(
        live.consume(&notification("root", "child", 2, created.clone()))
            .is_err()
    );
    let mut foreign = notification("foreign", "root", 1, created);
    foreign["params"]["rootSessionId"] = json!("elsewhere");
    assert!(live.consume(&foreign).unwrap().1.is_empty());
    assert_eq!(live.list().len(), 2);
    live.stopped();
    assert!(
        live.list()
            .iter()
            .all(|child| child.descriptor["status"] == "canceled")
    );
}

#[test]
fn legacy_updates_keep_native_tool_ids_and_terminal_events_flush_child_text() {
    let mut live = live::Live::new("root".into(), "/work".into(), ImageStore::default());
    let legacy = json!({"method":"session/update","params":{"sessionId":"root","update":{
        "sessionUpdate":"tool_call","toolCallId":"child:read","title":"Explore: read","status":"completed",
        "rawInput":{"path":"file"},"_meta":{"opencode/child-session":{"id":"child","parentID":"root","depth":1,"title":"Explore"}}}}});
    let (handled, events) = live.consume(&legacy).unwrap();
    assert!(handled);
    assert!(events.iter().any(|event| matches!(event, AgentTurnEvent::Subagent(SubagentEvent::Timeline { entry, .. }) if entry.item["callId"] == "read" && entry.item["name"] == "read")));
    live.consume(&notification("child", "root", 1, json!({"type":"update","update":{
        "sessionUpdate":"agent_message_chunk","messageId":"answer","content":{"type":"text","text":"Child answer"}}}))).unwrap();
    let (_, events) = live
        .consume(&notification(
            "child",
            "root",
            1,
            json!({"type":"status","status":"interrupted"}),
        ))
        .unwrap();
    assert!(events.iter().any(|event| matches!(event, AgentTurnEvent::Subagent(SubagentEvent::Timeline { entry, .. }) if entry.item["text"] == "Child answer")));
    assert_eq!(live.list()[0].descriptor["status"], "canceled");
    live.consume(&notification(
        "child",
        "root",
        1,
        json!({"type":"status","status":"running"}),
    ))
    .unwrap();
    live.consume(&notification(
        "child",
        "root",
        1,
        json!({"type":"status","status":"failed","error":{"message":"private"}}),
    ))
    .unwrap();
    assert_eq!(live.list()[0].descriptor["status"], "failed");
    assert!(!format!("{:?}", live.list()).contains("private"));
}

#[test]
fn task_links_require_structured_native_results_and_preserve_background_state() {
    let mut links = Links::default();
    links
        .update(
            &json!({"sessionUpdate":"tool_call","toolCallId":"task","name":"subagent",
        "rawInput":{"agent":"explore","description":"Review","prompt":"x".repeat(100_000)},"status":"pending"}),
        )
        .unwrap();
    assert!(
        links
            .update(
                &json!({"sessionUpdate":"tool_call_update","toolCallId":"task",
        "status":"completed","rawOutput":{"metadata":{"sessionID":"child","status":"running"}}})
            )
            .unwrap()
            .is_some()
    );
    let children = links.descriptors("parent", "/work").unwrap();
    assert_eq!(children[0].id, "child");
    assert_eq!(children[0].descriptor["status"], "running");
    assert_eq!(children[0].descriptor["subtitle"], "explore");
    assert!(links.descriptors("child", "/work").is_err());
    let mut unrelated = Links::default();
    unrelated
        .update(
            &json!({"sessionUpdate":"tool_call","toolCallId":"read","name":"read",
        "status":"completed","rawOutput":{"metadata":{"sessionID":"forged"}}}),
        )
        .unwrap();
    assert!(unrelated.children.is_empty());
}
