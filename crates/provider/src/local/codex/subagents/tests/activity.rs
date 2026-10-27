use super::*;

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
