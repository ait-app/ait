use super::*;

fn stream() -> Stream {
    let mut stream = Stream::default();
    stream.begin("turn".to_owned(), "conversation".to_owned());
    stream
}

fn step(index: u64, state: &str, kind: &str) -> Value {
    json!({"conversation_id":"conversation","step_index":index,"state":state,"step_type":kind})
}

fn result(status: &str) -> Value {
    json!({"conversation_id":"conversation","status":status,"response":"Hello world",
        "usage":{"input_tokens":100,"output_tokens":10,"cache_read_tokens":70}})
}

#[test]
fn emits_text_deltas_then_one_immutable_item_and_cumulative_usage() {
    let mut stream = stream();
    let mut update = step(2, "ACTIVE", "agent_response");
    update["text_delta"] = json!("Hello ");
    stream.update(&update).unwrap();
    assert!(
        matches!(stream.events.pop_front(), Some(AgentTurnEvent::Progress { entry, .. })
        if entry.item["text"] == "Hello ")
    );
    update["state"] = json!("DONE");
    update["text_delta"] = json!("world");
    stream.update(&update).unwrap();
    assert!(
        matches!(stream.events.pop_front(), Some(AgentTurnEvent::Timeline(entry))
        if entry.item["text"] == "Hello world"
            && entry.key == "native:antigravity:conversation:turn:step:2")
    );
    stream.finish(&result("SUCCESS")).unwrap();
    assert!(
        matches!(stream.events.pop_front(), Some(AgentTurnEvent::Usage(usage))
        if usage.input_tokens == Some(100) && usage.cached_input_tokens == Some(70))
    );
    assert_eq!(
        stream.events.pop_front(),
        Some(AgentTurnEvent::Completed(Some("Hello world".to_owned())))
    );
    assert!(stream.events.is_empty());
}

#[test]
fn projects_tool_snapshots_errors_and_unknown_tools() {
    let mut stream = stream();
    let mut update = step(3, "ACTIVE", "tool");
    update["tool_name"] = json!("run_command");
    update["tool_info"] = json!({"parameters":{"CommandLine":"echo hello","Cwd":"/workspace"}});
    stream.update(&update).unwrap();
    update["state"] = json!("DONE");
    update["tool_info"] = json!({"output":"hello","error":{"type":"denied","message":"Denied"}});
    stream.update(&update).unwrap();
    assert!(
        matches!(stream.events.pop_front(), Some(AgentTurnEvent::Progress { entry, .. })
        if entry.item["status"] == "running")
    );
    let Some(AgentTurnEvent::Timeline(entry)) = stream.events.pop_front() else {
        panic!("tool")
    };
    assert_eq!(entry.item["status"], "failed");
    assert_eq!(entry.item["detail"]["command"], "echo hello");
    assert_eq!(entry.item["detail"]["output"], "hello");
    assert_eq!(entry.item["detail"]["cwd"], "/workspace");
    assert_eq!(
        tool_detail(&json!({"name":"custom","parameters":{"x":1}}))["type"],
        "unknown"
    );
}

#[test]
fn terminal_statuses_do_not_turn_errors_or_waiting_into_success() {
    for status in [
        "SUCCESS",
        "ERROR",
        "CANCELED",
        "INTERRUPTED",
        "INVALID",
        "WAITING",
        "RUNNING",
    ] {
        let mut stream = stream();
        stream.finish(&result(status)).unwrap();
        let terminal = stream.events.pop_back().unwrap();
        assert_eq!(
            terminal,
            match status {
                "SUCCESS" => AgentTurnEvent::Completed(Some("Hello world".to_owned())),
                "CANCELED" | "INTERRUPTED" => AgentTurnEvent::Cancelled,
                _ => AgentTurnEvent::Failed,
            }
        );
    }
}

#[test]
fn rejects_foreign_duplicate_unfinished_and_oversized_events() {
    let mut stream = stream();
    let mut update = step(1, "DONE", "agent_response");
    update["conversation_id"] = json!("foreign");
    assert!(stream.update(&update).is_err());
    update["conversation_id"] = json!("conversation");
    stream.update(&update).unwrap();
    assert!(stream.update(&update).is_err());
    let mut partial = step(2, "ACTIVE", "agent_response");
    partial["text_delta"] = json!("partial");
    stream.update(&partial).unwrap();
    assert!(stream.finish(&result("SUCCESS")).is_err());
    partial["text_delta"] = json!("x".repeat(MAX_TEXT + 1));
    assert!(stream.update(&partial).is_err());
    let mut invalid = result("SUCCESS");
    invalid["usage"]["input_tokens"] = json!(-1);
    assert!(stream.finish(&invalid).is_err());
}

#[test]
fn ignores_native_user_checkpoint_and_future_categories() {
    let mut stream = stream();
    for kind in ["user_input", "checkpoint", "future"] {
        stream.update(&step(1, "DONE", kind)).unwrap();
    }
    assert!(stream.events.is_empty());
}

#[test]
fn caps_large_tool_output_at_a_utf8_boundary() {
    let (projected, bytes) = preview(&json!("界".repeat(20_000)));
    let text = projected.as_str().unwrap();
    assert!(bytes <= 32 * 1024);
    assert!(text.ends_with("[Output truncated; full output remains in AGY.]"));
    assert!(text.len() < 33 * 1024);
}

#[test]
fn native_success_with_denied_actions_finalizes_the_tool_as_failed() {
    for tool_done in [false, true] {
        let mut stream = stream();
        let mut update = step(2, "ACTIVE", "tool");
        update["tool_name"] = json!("run_command");
        update["tool_info"] = json!({"parameters":{"CommandLine":"printf diagnostic"}});
        stream.update(&update).unwrap();
        if tool_done {
            update["state"] = json!("DONE");
            stream.update(&update).unwrap();
        }
        assert!(
            stream
                .events
                .iter()
                .all(|event| !matches!(event, AgentTurnEvent::Timeline(_)))
        );
        let mut result = result("SUCCESS");
        result["response"] = json!("");
        result["denied_actions"] = json!([{"action":"command","display_name":"RunCommand"}]);
        stream.finish(&result).unwrap();
        let tool = stream
            .events
            .iter()
            .find_map(|event| match event {
                AgentTurnEvent::Timeline(entry) => Some(&entry.item),
                _ => None,
            })
            .unwrap();
        assert_eq!(tool["status"], "failed");
        assert_eq!(tool["error"]["message"], Failure::Permission.message());
        assert_eq!(tool["detail"]["command"], "printf diagnostic");
        assert_eq!(stream.failure, Some(Failure::Permission));
        assert_eq!(stream.events.back(), Some(&AgentTurnEvent::Failed));
    }
}

#[test]
fn denied_tools_can_coexist_with_a_successful_response_and_other_empty_tools() {
    let mut stream = stream();
    stream.update(&step(1, "DONE", "agent_response")).unwrap();
    for (index, name) in [(2, "run_command"), (3, "write_to_file")] {
        let mut update = step(index, "DONE", "tool");
        update["tool_name"] = json!(name);
        stream.update(&update).unwrap();
    }
    let mut result = result("SUCCESS");
    result["denied_actions"] = json!([{"action":"command","display_name":"RunCommand"}]);
    stream.finish(&result).unwrap();
    let tools: Vec<_> = stream
        .events
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Timeline(entry) if entry.item["type"] == "tool_call" => {
                Some(&entry.item)
            }
            _ => None,
        })
        .collect();
    assert_eq!(tools[0]["status"], "failed");
    assert_eq!(tools[1]["status"], "completed");
    assert!(stream.events.iter().any(|event| matches!(event,
        AgentTurnEvent::Timeline(entry) if entry.item["text"] == "Hello world")));
    assert_eq!(stream.failure, None);
    assert_eq!(
        stream.events.back(),
        Some(&AgentTurnEvent::Completed(Some("Hello world".to_owned())))
    );
    assert_eq!(
        normalized_tool_name("WriteToFile"),
        normalized_tool_name("write_to_file")
    );
}

#[test]
fn rejects_malformed_or_oversized_denial_lists() {
    for denied in [
        json!({}),
        json!([{}]),
        json!([{"display_name":"bad\nname"}]),
        json!([{"display_name":"x".repeat(513)}]),
        json!(vec![json!({"display_name":"RunCommand"}); MAX_ITEMS + 1]),
    ] {
        assert!(denied_tools(&json!({"denied_actions":denied})).is_err());
    }
    assert!(
        denied_tools(&json!({"denied_actions":[]}))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn stream_failure_commits_partial_tools_with_an_explanation() {
    let mut stream = stream();
    let mut update = step(1, "ACTIVE", "tool");
    update["tool_name"] = json!("run_command");
    stream.update(&update).unwrap();
    stream.abort(Failure::Protocol);
    assert!(stream.events.iter().any(|event| matches!(event,
        AgentTurnEvent::Timeline(entry) if entry.item["status"] == "failed"
        && entry.item["error"]["message"] == Failure::Protocol.message())));
    assert_eq!(stream.events.back(), Some(&AgentTurnEvent::Failed));
}

#[test]
fn cancellation_closes_running_tools_without_reporting_a_provider_failure() {
    let mut stream = stream();
    let mut update = step(1, "ACTIVE", "tool");
    update["tool_name"] = json!("run_command");
    stream.update(&update).unwrap();
    stream.finish(&result("INTERRUPTED")).unwrap();
    assert!(stream.events.iter().any(|event| matches!(event,
        AgentTurnEvent::Timeline(entry) if entry.item["status"] == "canceled"
        && entry.item["error"].is_null())));
    assert_eq!(stream.failure, None);
    assert_eq!(stream.events.back(), Some(&AgentTurnEvent::Cancelled));
}
