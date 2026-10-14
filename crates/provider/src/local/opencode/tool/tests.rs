use serde_json::{Value, json};

#[test]
fn native_tool_projections_match_the_shared_wire_contract_cases() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/opencode_tool_cards.json"
    ))
    .unwrap();
    for case in cases {
        assert_eq!(
            super::detail(&case["snapshot"]),
            case["detail"],
            "{}",
            case["name"]
        );
        let mut stream = super::super::streaming::Stream::default();
        let mut update = case["snapshot"].clone();
        update["sessionUpdate"] = json!("tool_call");
        update["toolCallId"] = json!("tool");
        update["status"] = json!("completed");
        stream.update(&update).unwrap();
        let entry = stream
            .events
            .into_iter()
            .find_map(|event| {
                if let crate::ports::agent_session::AgentTurnEvent::Timeline(entry) = event {
                    Some(entry)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(entry.item["detail"], case["detail"], "{}", case["name"]);
    }
}

#[test]
fn text_cards_do_not_stringify_structured_tool_metadata() {
    let output = json!({"result":true});
    assert!(
        super::detail(&json!({"kind":"execute","rawInput":{"command":"pwd"},"rawOutput":output}))
            .get("output")
            .is_none()
    );
    assert_eq!(super::raw_output(&output), &output);
    assert_eq!(
        super::raw_output(&json!({"output":"text","metadata":{}})),
        "text"
    );
    assert_eq!(
        super::detail(&json!({"kind":"read","rawInput":{"path":"file"},"content":[]})),
        json!({"type":"read","filePath":"file"})
    );
}

#[test]
fn native_acp_shell_read_edit_and_other_tools_preserve_cards() {
    for (snapshot, expected) in [
        (
            json!({"kind":"execute","rawInput":{"command":"pwd","cwd":"/work"},"rawOutput":"/work"}),
            json!({"type":"shell","command":"pwd","cwd":"/work","output":"/work"}),
        ),
        (
            json!({"kind":"read","rawInput":{"path":"file","offset":3},"rawOutput":"text"}),
            json!({"type":"read","filePath":"file","offset":3,"content":"text"}),
        ),
        (
            json!({"kind":"edit","rawInput":{"filePath":"file","oldString":"old","newString":"new"}}),
            json!({"type":"edit","filePath":"file","oldString":"old","newString":"new"}),
        ),
        (
            json!({"kind":"other","rawInput":{"argument":"value"},"rawOutput":"result"}),
            json!({"type":"unknown","input":{"argument":"value"},"output":"result"}),
        ),
    ] {
        assert_eq!(super::detail(&snapshot), expected);
    }
}
