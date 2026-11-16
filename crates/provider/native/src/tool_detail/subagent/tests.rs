use super::*;

#[test]
fn native_agent_paths_keep_nested_names_and_link_to_the_child_session() {
    for (path, expected) in [
        ("/root/code_reviewer", "Code reviewer"),
        ("/root/review/unit_tests", "Review / Unit tests"),
        ("C:\\agents\\code-reviewer", "Code reviewer"),
        ("/root", "Sub-agent"),
    ] {
        let item = json!({"agentThreadId":"child", "agentPath":path,"kind":"started"});
        let detail = activity(&item).unwrap();
        assert_eq!(detail["type"], "sub_agent");
        assert_eq!(detail["childSessionId"], "child");
        assert_eq!(detail["subAgentType"], expected);
    }
    assert!(
        activity(&json!({"agentThreadId":"child","agentPath":"/root/test","kind":"unknown"}))
            .is_none()
    );
}
