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
