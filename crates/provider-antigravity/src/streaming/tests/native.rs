use super::*;

fn replay(capture: &str) -> Vec<AgentTurnEvent> {
    let mut stream = Stream::default();
    let mut turn = 0;
    let mut events = Vec::new();
    for line in capture.lines() {
        let row: Value = serde_json::from_str(line).unwrap();
        let frame = &row["data"];
        match frame["event"].as_str().unwrap() {
            "init" => {}
            "step_update" => {
                let update = &frame["step_update"];
                if update["step_type"] == "user_input" {
                    turn += 1;
                    stream.begin(
                        format!("turn-{turn}"),
                        update["conversation_id"].as_str().unwrap().to_owned(),
                    );
                }
                stream.update(update).expect("recorded native step");
            }
            "result" => stream.finish(&frame["result"]).expect("recorded result"),
            other => panic!("unexpected capture event {other}"),
        }
        events.extend(stream.events.drain(..));
    }
    events
}

#[test]
fn recorded_interrupted_error_cancels_the_turn_and_running_tool() {
    let events = replay(include_str!(
        "../../tests/fixtures/native/sigint-mid-tool.ndjson"
    ));
    assert_eq!(events.last(), Some(&AgentTurnEvent::Cancelled));
    assert!(events.iter().any(|event| matches!(event,
        AgentTurnEvent::Timeline(entry) if entry.item["name"] == "run_command"
            && entry.item["status"] == "canceled" && entry.item["error"].is_null())));
}

#[test]
fn recorded_error_steps_preserve_the_denial_and_allow_the_next_turn() {
    let events = replay(include_str!(
        "../../tests/fixtures/native/denial-replay.ndjson"
    ));
    assert!(events.iter().any(|event| matches!(event,
        AgentTurnEvent::Timeline(entry) if entry.item["name"] == "run_command"
            && entry.item["status"] == "failed"
            && entry.item["error"]["message"].as_str().unwrap().contains("permission check failed"))));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, AgentTurnEvent::Failed))
            .count(),
        1
    );
    assert_eq!(
        events.last(),
        Some(&AgentTurnEvent::Completed(Some("FOLLOWUP_OK\n".to_owned())))
    );
}

#[test]
fn recorded_subagent_steps_expose_the_child_conversation() {
    let events = replay(include_str!("../../tests/fixtures/native/subagent.ndjson"));
    assert!(events.iter().any(|event| matches!(event,
        AgentTurnEvent::Timeline(entry) if entry.item["name"] == "invoke_subagent"
            && entry.item["status"] == "completed"
            && entry.item["detail"]["type"] == "sub_agent"
            && entry.item["detail"]["childSessionId"] == "b6f02329-7415-49a7-a9a9-1cd09e898d26")));
}

#[test]
fn recorded_file_tools_use_native_paths_and_do_not_invent_write_contents() {
    let events = replay(include_str!(
        "../../tests/fixtures/native/tools-default.ndjson"
    ));
    for (name, kind) in [("view_file", "read"), ("write_to_file", "write")] {
        assert!(
            events.iter().any(|event| matches!(event,
            AgentTurnEvent::Timeline(entry) if entry.item["name"] == name
                && entry.item["detail"]["type"] == kind
                && entry.item["detail"]["filePath"].is_string())),
            "{name}"
        );
    }
    assert!(
        events
            .iter()
            .filter_map(|event| match event {
                AgentTurnEvent::Timeline(entry) if entry.item["detail"]["type"] == "write" =>
                    Some(entry),
                _ => None,
            })
            .all(|entry| entry.item["detail"].get("content").is_none())
    );
}

#[test]
fn cumulative_denials_only_fail_new_occurrences() {
    let mut stream = stream();
    for (turn, count, failed) in [(1, 1, true), (2, 1, false), (3, 2, true), (4, 2, false)] {
        stream.begin(format!("turn-{turn}"), "conversation".to_owned());
        let mut update = step(2, "DONE", "tool");
        update["tool_name"] = json!("run_command");
        stream.update(&update).unwrap();
        let mut result = result("SUCCESS");
        result["response"] = json!("");
        result["denied_actions"] = json!(vec![
            json!({"action":"command","display_name":"RunCommand"});
            count
        ]);
        stream.finish(&result).unwrap();
        assert_eq!(
            stream.events.pop_back(),
            Some(if failed {
                AgentTurnEvent::Failed
            } else {
                AgentTurnEvent::Completed(Some(String::new()))
            }),
            "turn {turn}"
        );
        assert!(stream.events.iter().any(|event| matches!(event,
            AgentTurnEvent::Timeline(entry) if entry.item["status"] == if failed { "failed" } else { "completed" })));
        stream.events.clear();
    }
}

#[test]
fn error_tool_without_error_payload_still_has_a_valid_failed_card() {
    let mut stream = stream();
    let mut update = step(1, "ERROR", "tool");
    update["tool_name"] = json!("run_command");
    update["tool_info"] = Value::Null;
    stream.update(&update).unwrap();
    assert!(
        matches!(stream.events.pop_front(), Some(AgentTurnEvent::Timeline(entry))
        if entry.item["status"] == "failed" && !entry.item["error"].is_null())
    );
}

#[test]
fn denial_counts_reset_only_when_the_native_process_changes() {
    let mut stream = stream();
    let mut result = result("SUCCESS");
    result["response"] = json!("");
    result["denied_actions"] = json!([{"action":"command","display_name":"RunCommand"}]);
    stream.finish(&result).unwrap();
    stream.events.clear();
    stream.reset_process();
    stream.begin("next".to_owned(), "conversation".to_owned());
    stream.finish(&result).unwrap();
    assert_eq!(stream.events.back(), Some(&AgentTurnEvent::Failed));
}

#[test]
fn active_tools_do_not_publish_a_terminal_error_early() {
    let mut stream = stream();
    let mut update = step(1, "ACTIVE", "tool");
    update["tool_name"] = json!("run_command");
    update["tool_info"] = json!({"error":{"message":"native error"}});
    stream.update(&update).unwrap();
    assert!(
        matches!(stream.events.pop_front(), Some(AgentTurnEvent::Progress { entry, .. })
        if entry.item["status"] == "running" && entry.item["error"].is_null())
    );
    update["state"] = json!("ERROR");
    stream.update(&update).unwrap();
    assert!(
        matches!(stream.events.pop_front(), Some(AgentTurnEvent::Timeline(entry))
        if entry.item["status"] == "failed" && entry.item["error"]["message"] == "native error")
    );
}

#[test]
fn error_checkpoints_are_not_reported_as_completed() {
    let mut stream = stream();
    stream.update(&step(1, "ACTIVE", "checkpoint")).unwrap();
    stream.update(&step(1, "ERROR", "checkpoint")).unwrap();
    assert!(
        matches!(stream.events.back(), Some(AgentTurnEvent::Timeline(entry))
        if entry.item["type"] == "compaction" && entry.item["status"] == "failed")
    );
}
