use serde_json::json;

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
            json!({"type":"edit","filePath":"file","oldString":"old","newString":"new","output":null}),
        ),
        (
            json!({"kind":"other","rawInput":{"argument":"value"},"rawOutput":"result"}),
            json!({"type":"unknown","input":{"argument":"value"},"output":"result"}),
        ),
    ] {
        assert_eq!(super::detail(&snapshot), expected);
    }
}
