use super::*;

#[test]
fn child_command_actions_drain_all_progress_and_match_completed_history() {
    let mut live = Live::default();
    let images = ImageStore::default();
    live.observe(
        "item/completed",
        &spawn("root", "child"),
        ("root", "/tmp"),
        &images,
    )
    .unwrap();
    let mut native = json!({"threadId":"child","turnId":"child-turn","item":{
    "type":"commandExecution","id":"tool","status":"inProgress",
    "command":"/bin/zsh -lc 'echo first; echo second'",
    "commandActions":[
        {"type":"unknown","command":"echo first"},
        {"type":"unknown","command":"echo second"}
    ]}});
    let started = live
        .observe("item/started", &native, ("root", "/tmp"), &images)
        .unwrap();
    let keys = started
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Subagent(SubagentEvent::Progress { id, entry, .. }) => {
                assert_eq!(id, "child");
                Some(entry.key.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        keys,
        [
            "native:child-turn:tool:0",
            "native:child-turn:tool:1",
            "native:child-turn:tool:output"
        ]
    );
    let updates = live
        .observe(
            "item/commandExecution/outputDelta",
            &json!({
                "threadId":"child","turnId":"child-turn","itemId":"tool","delta":"output"
            }),
            ("root", "/tmp"),
            &images,
        )
        .unwrap();
    assert_eq!(updates.len(), 1);
    assert!(updates.iter().all(|event| matches!(event,
        AgentTurnEvent::Subagent(SubagentEvent::Progress {entry,..}) if entry.item["detail"]["output"] == "output")));
    native["item"]["status"] = json!("completed");
    native["item"]["aggregatedOutput"] = json!("output");
    let completed = live
        .observe("item/completed", &native, ("root", "/tmp"), &images)
        .unwrap();
    let final_keys = completed
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Subagent(SubagentEvent::Timeline { entry, .. }) => {
                Some(entry.key.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(final_keys, keys);
    assert_eq!(
        live.observe("item/completed", &native, ("root", "/tmp"), &images)
            .unwrap(),
        [] as [AgentTurnEvent; 0]
    );
}
