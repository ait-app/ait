use super::*;

#[test]
fn completion_updates_only_known_children_and_is_idempotent_in_either_event_order() {
    let images = ImageStore::default();
    for completion_first in [false, true] {
        let mut live = Live::default();
        live.observe(
            "item/completed",
            &spawn("root", "child"),
            ("root", "/tmp"),
            &images,
        )
        .unwrap();
        let activity = json!({"threadId":"root","turnId":"parent-turn","item":{
            "type":"subAgentActivity","id":"subagent-completed-child-turn","kind":"completed",
            "agentThreadId":"child","agentPath":"/root/reviewer","model":null,"reasoningEffort":null}});
        let completed = json!({"threadId":"child","turn":{"id":"child-turn","status":"completed"}});
        let mut notifications = [
            ("item/completed", &activity),
            ("turn/completed", &completed),
        ];
        if !completion_first {
            notifications.reverse();
        }
        for (method, params) in notifications {
            let events = live
                .observe(method, params, ("root", "/tmp"), &images)
                .unwrap();
            assert!(
                events
                    .iter()
                    .all(|event| matches!(event, AgentTurnEvent::Subagent(_)))
            );
            assert_eq!(live.children()[0].descriptor["status"], "completed");
        }
        assert!(
            live.observe("item/completed", &activity, ("root", "/tmp"), &images)
                .unwrap()
                .is_empty()
        );
        assert_eq!(live.children()[0].parent_id, "root");

        let mut unknown = activity;
        unknown["item"]["agentThreadId"] = json!("unknown");
        assert!(
            live.observe("item/completed", &unknown, ("root", "/tmp"), &images)
                .unwrap()
                .is_empty()
        );
        assert_eq!(live.children().len(), 1);
        assert!(!live.contains("unknown"));
    }
}

#[test]
fn completion_preserves_a_childs_native_failed_or_canceled_status() {
    let images = ImageStore::default();
    for (native_status, expected) in [("failed", "failed"), ("interrupted", "canceled")] {
        let mut live = Live::default();
        live.observe(
            "item/completed",
            &spawn("root", "child"),
            ("root", "/tmp"),
            &images,
        )
        .unwrap();
        live.observe(
            "turn/completed",
            &json!({"threadId":"child","turn":{"id":"child-turn","status":native_status}}),
            ("root", "/tmp"),
            &images,
        )
        .unwrap();
        let completion = json!({"threadId":"root","turnId":"parent-turn","item":{
            "type":"subAgentActivity","id":"child-completed","kind":"completed",
            "agentThreadId":"child","agentPath":"/root/reviewer"}});
        assert!(
            live.observe("item/completed", &completion, ("root", "/tmp"), &images)
                .unwrap()
                .is_empty()
        );
        assert_eq!(live.children()[0].descriptor["status"], expected);
    }
}

#[test]
fn child_interactions_preserve_ancestry_without_enrolling_their_targets() {
    let mut live = Live::default();
    let images = ImageStore::default();
    for (parent, child) in [("root", "child"), ("root", "sibling"), ("child", "nested")] {
        live.observe(
            "item/completed",
            &spawn(parent, child),
            ("root", "/tmp"),
            &images,
        )
        .unwrap();
    }
    let descendants = live.children();

    for receiver in ["root", "child", "sibling", "unknown"] {
        let activity = json!({"threadId":"nested","turnId":"nested-turn","item":{
            "id":format!("message-{receiver}"),"type":"subAgentActivity","kind":"interacted",
            "agentThreadId":receiver,"agentPath":format!("/root/{receiver}")}});
        for method in ["item/started", "item/completed"] {
            let events = live
                .observe(method, &activity, ("root", "/tmp"), &images)
                .unwrap();
            assert!(
                events.iter().all(|event| !matches!(
                    event,
                    AgentTurnEvent::Subagent(SubagentEvent::Upsert(_))
                ))
            );
        }
    }

    assert_eq!(live.children(), descendants);
    assert!(!live.contains("root"));
    assert!(!live.contains("unknown"));
}

#[test]
fn interruptions_update_only_known_descendants_without_creating_ancestry() {
    let mut live = Live::default();
    let images = ImageStore::default();
    for child in ["sender", "target"] {
        live.observe(
            "item/completed",
            &spawn("root", child),
            ("root", "/tmp"),
            &images,
        )
        .unwrap();
    }

    for receiver in ["root", "target", "unknown"] {
        let activity = json!({"threadId":"sender","turnId":"sender-turn","item":{
            "id":format!("interrupt-{receiver}"),"type":"subAgentActivity","kind":"interrupted",
            "agentThreadId":receiver,"agentPath":format!("/root/{receiver}")}});
        live.observe("item/completed", &activity, ("root", "/tmp"), &images)
            .unwrap();
    }

    let descendants = live.children();
    assert_eq!(descendants.len(), 2);
    assert!(descendants.iter().all(|child| child.parent_id == "root"));
    assert_eq!(descendants[0].descriptor["status"], "running");
    assert_eq!(descendants[1].descriptor["status"], "canceled");
}

#[test]
fn modern_activity_preserves_ancestry_interruption_and_child_only_display_events() {
    let mut live = Live::default();
    let images = ImageStore::default();
    let mut activity = json!({"threadId":"root","turnId":"root-turn","item":{
        "id":"spawn","type":"subAgentActivity","kind":"started","agentThreadId":"child",
        "agentPath":"/root/reviewer","model":"offline","reasoningEffort":"high"}});
    live.observe("item/completed", &activity, ("root", "/tmp"), &images)
        .unwrap();
    assert_eq!(live.children()[0].parent_id, "root");
    assert_eq!(live.children()[0].descriptor["title"], "Reviewer");
    assert_eq!(live.children()[0].descriptor["toolCallId"], "spawn");
    let child = json!({"threadId":"child","turnId":"child-turn","item":{
        "type":"contextCompaction","id":"compact"}});
    let events = live
        .observe("item/started", &child, ("root", "/tmp"), &images)
        .unwrap();
    assert!(events.iter().any(
        |event| matches!(event, AgentTurnEvent::Subagent(SubagentEvent::Timeline {id,entry})
        if id == "child" && entry.item["status"] == "loading")
    ));
    let events = live
        .observe(
            "item/commandExecution/terminalInteraction",
            &json!({"threadId":"child","turnId":"child-turn","processId":"42","stdin":"answer"}),
            ("root", "/tmp"),
            &images,
        )
        .unwrap();
    assert!(events.iter().any(
        |event| matches!(event, AgentTurnEvent::Subagent(SubagentEvent::Timeline {id,entry})
        if id == "child" && entry.item["name"] == "terminal")
    ));
    let events = live
        .observe(
            "turn/completed",
            &json!({"threadId":"child",
        "turn":{"id":"child-turn","status":"completed"}}),
            ("root", "/tmp"),
            &images,
        )
        .unwrap();
    assert!(events.iter().any(
        |event| matches!(event, AgentTurnEvent::Subagent(SubagentEvent::Timeline {entry,..})
        if entry.item["type"] == "compaction" && entry.item["status"] == "completed")
    ));
    activity["item"]["kind"] = json!("interacted");
    live.observe("item/completed", &activity, ("root", "/tmp"), &images)
        .unwrap();
    assert_eq!(live.children()[0].descriptor["status"], "completed");
    activity["item"]["kind"] = json!("interrupted");
    live.observe("item/completed", &activity, ("root", "/tmp"), &images)
        .unwrap();
    assert_eq!(live.children()[0].descriptor["status"], "canceled");
    activity["item"]["agentThreadId"] = json!("root");
    assert!(
        live.observe("item/completed", &activity, ("root", "/tmp"), &images)
            .is_err()
    );
}
