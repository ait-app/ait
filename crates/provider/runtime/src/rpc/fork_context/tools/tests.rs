use super::*;
use serde_json::json;

#[test]
fn summaries_follow_paseo_canonical_details_and_never_include_raw_input() {
    for (detail, expected) in [
        (
            json!({"type":"read","filePath":"src/main.rs","content":"secret"}),
            "[Read] src/main.rs",
        ),
        (json!({"type":"write","filePath":"a"}), "[Write] a"),
        (json!({"type":"edit","filePath":"a"}), "[Edit] a"),
        (
            json!({"type":"shell","command":"cargo\n test","output":"secret"}),
            "[Shell] cargo test",
        ),
        (json!({"type":"search","query":"needle"}), "[Search] needle"),
        (
            json!({"type":"fetch","url":"https://example.invalid"}),
            "[Fetch] https://example.invalid",
        ),
        (
            json!({"type":"worktree_setup","branchName":"feature"}),
            "[Worktree setup] feature",
        ),
        (
            json!({"type":"sub_agent","subAgentType":"Explore","description":"Read repo"}),
            "[Explore] Read repo",
        ),
        (
            json!({"type":"sub_agent","subAgentType":"","description":""}),
            "[Task]",
        ),
        (
            json!({"type":"plain_text","label":"first\n\n second"}),
            "[Custom tool] first second",
        ),
        (json!({"type":"plan"}), "[Plan]"),
        (
            json!({"type":"unknown","input":"secret","output":"secret"}),
            "[Custom tool]",
        ),
    ] {
        assert_eq!(
            summary(&json!({"name":"custom_tool","detail":detail})),
            expected
        );
    }
    assert_eq!(
        summary(&json!({"name":"terminal","detail":{"type":"plain_text","label":"cargo\n test"}})),
        "[Terminal] cargo test"
    );
    assert_eq!(
        summary(
            &json!({"name":"Task","detail":{"type":"unknown"},"metadata":{"subAgentActivity":"running"}})
        ),
        "[Task] running"
    );
}

#[test]
fn tool_names_preserve_external_namespaces_and_humanize_ait_tools() {
    for (name, expected) in [
        ("exec_command", "Exec command"),
        ("read-file", "Read file"),
        ("mcp__ait__create_agent", "Create agent"),
        ("mcp__ait_host__wait_for_agent", "Wait for agent"),
        ("ait.create_agent", "Create agent"),
        ("ait_host.create_agent", "Create agent"),
        ("mcp__ait__speak", "mcp__ait__speak"),
        ("mcp__aitx__create_agent", "mcp__aitx__create_agent"),
        ("paseo__create_agent", "paseo__create_agent"),
        ("mcp__paseo__create_agent", "mcp__paseo__create_agent"),
        (
            "mcp__paseo_host__create_agent",
            "mcp__paseo_host__create_agent",
        ),
        ("paseo.create_agent", "paseo.create_agent"),
        ("paseo_host.create_agent", "paseo_host.create_agent"),
        ("mcp__paseo__speak", "mcp__paseo__speak"),
        ("mcp__external__read", "mcp__external__read"),
        ("custom/tool", "custom/tool"),
        ("  ", "  "),
    ] {
        assert_eq!(humanize(name), expected);
    }
}

#[test]
fn summaries_are_bounded_by_utf16_units() {
    let text =
        summary(&json!({"name":"tool","detail":{"type":"plain_text","label":"x".repeat(201)}}));
    assert_eq!(text, format!("[Tool] {}...", "x".repeat(197)));
    let text =
        summary(&json!({"name":"tool","detail":{"type":"plain_text","label":"界".repeat(200)}}));
    assert!(!text.ends_with("..."));
}
