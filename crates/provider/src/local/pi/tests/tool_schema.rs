use super::*;

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ToolCase {
    name: String,
    events: Vec<Value>,
    host_action: Option<HostAction>,
    item: Value,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum HostAction {
    Fail,
}

#[test]
fn tool_events_match_the_shared_client_schema_fixtures() {
    let cases: Vec<ToolCase> = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/pi_tool_cards.json"
    ))
    .unwrap();
    for case in cases {
        let mut stream = streaming::Stream::default();
        stream.begin("turn".into());
        for event in &case.events {
            stream
                .consume(event)
                .unwrap_or_else(|error| panic!("{}: {error:?}", case.name));
        }
        // Interruption is a host action, not an invented Pi wire event.
        if let Some(HostAction::Fail) = case.host_action {
            stream.fail();
        }
        let items: Vec<_> = stream
            .events
            .iter()
            .filter_map(|event| match event {
                AgentTurnEvent::Progress { entry, .. } | AgentTurnEvent::Timeline(entry) => {
                    Some(&entry.item)
                }
                _ => None,
            })
            .collect();
        assert_eq!(items, [&case.item], "{}", case.name);
    }
}

#[test]
fn shell_results_extract_text_before_bounding_the_preview() {
    let mut stream = streaming::Stream::default();
    stream.begin("turn".into());
    stream
        .consume(&json!({"type":"tool_execution_start","toolCallId":"call",
        "toolName":"bash","args":{"command":"pwd","timeout":30}}))
        .unwrap();
    stream
        .consume(&json!({"type":"tool_execution_end","toolCallId":"call",
        "toolName":"bash","result":{"content":[
            {"type":"image","data":"ignored"},
            {"type":"text","text":"first"},
            {"type":"text","text":"界".repeat(20_000)}
        ],"details":{"metadata":"hidden"}},"isError":false}))
        .unwrap();
    let detail = stream
        .events
        .iter()
        .find_map(|event| match event {
            AgentTurnEvent::Timeline(entry) => Some(&entry.item["detail"]),
            _ => None,
        })
        .unwrap();
    assert_eq!(detail["type"], "shell");
    assert_eq!(detail["command"], "pwd");
    let output = detail["output"].as_str().unwrap();
    assert!(output.starts_with("first\n界"));
    assert!(output.ends_with("[Output truncated; full output remains in the native transcript.]"));
    assert!(output.len() < 33 * 1024);
}

#[test]
fn large_edit_keeps_structured_changes_with_bounded_unicode_previews() {
    let mut stream = streaming::Stream::default();
    stream.begin("turn".into());
    stream
        .consume(&json!({"type":"tool_execution_start","toolCallId":"call",
        "toolName":"edit","args":{"path":"src/main.rs",
            "oldText":"界".repeat(20_000),"newText":""}}))
        .unwrap();
    stream.fail();
    let detail = stream
        .events
        .iter()
        .find_map(|event| match event {
            AgentTurnEvent::Timeline(entry) => Some(&entry.item["detail"]),
            _ => None,
        })
        .unwrap();
    assert_eq!(detail["type"], "edit");
    assert_eq!(detail["filePath"], "src/main.rs");
    assert_eq!(detail["newString"], "");
    let old = detail["oldString"].as_str().unwrap();
    assert!(old.starts_with('界'));
    assert!(old.ends_with("[Output truncated; full output remains in the native transcript.]"));
    assert!(old.len() < 33 * 1024);
}

#[test]
fn malformed_tool_metadata_degrades_without_failing_the_turn() {
    let mut stream = streaming::Stream::default();
    stream.begin("turn".into());
    stream
        .consume(&json!({"type":"tool_execution_start","toolCallId":"call",
            "toolName":42,"args":["unexpected"]}))
        .unwrap();
    stream
        .consume(&json!({"type":"tool_execution_end","toolCallId":"call",
            "result":{"unexpected":"retained"},"isError":false}))
        .unwrap();
    assert!(
        stream
            .consume(&json!({"type":"agent_settled","aborted":false}))
            .unwrap()
    );
    let Some(AgentTurnEvent::Timeline(entry)) = stream.events.front() else {
        panic!("expected completed fallback card");
    };
    assert_eq!(entry.item["name"], "unknown");
    assert_eq!(
        entry.item["detail"],
        json!({"type":"unknown",
        "input":["unexpected"],"output":{"unexpected":"retained"}})
    );
    assert!(matches!(
        stream.events.back(),
        Some(AgentTurnEvent::Completed(_))
    ));
}
