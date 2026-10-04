use super::*;

#[test]
fn native_shell_file_web_and_delegation_tools_get_typed_details() {
    let shell = codex(
        &json!({"type":"commandExecution","command":"pwd","cwd":"/project","aggregatedOutput":"/project","exitCode":0}),
    );
    assert_eq!(
        shell,
        json!({"type":"shell","command":"pwd","cwd":"/project","output":"/project","exitCode":0})
    );
    let edit = codex(&json!({"type":"fileChange","changes":[{"path":"a.rs","diff":"-old\n+new"}]}));
    assert_eq!(
        edit,
        json!({"type":"edit","filePath":"a.rs","unifiedDiff":"-old\n+new"})
    );
    assert_eq!(
        codex(&json!({"type":"fileChange","changes":[]}))["type"],
        "unknown"
    );
    assert_eq!(
        codex(&json!({"type":"webSearch","action":{"query":"Rust"}}))["query"],
        "Rust"
    );
    assert_eq!(
        codex(&json!({"type":"webSearch","action":{"url":"https://example.com"}}))["type"],
        "fetch"
    );
    assert_eq!(
        codex(
            &json!({"type":"collabAgentToolCall","receiverThreadIds":["child"],"prompt":"Review"})
        )["childSessionId"],
        "child"
    );
}

#[test]
fn codex_mcp_tool_projects_arguments_and_result_without_native_envelope() {
    let native = json!({"id":"call","type":"mcpToolCall","server":"cua_repl","tool":"js",
        "status":"inProgress","arguments":{"code":"await tab.goto('https://example.com')"},
        "result":null,"pluginId":"unified-computer-use@openai-bundled","readOnlyHint":true});
    let running = codex_tools(&native, "running");
    assert_eq!(running.len(), 1);
    assert_eq!(running[0].0, "call");
    assert_eq!(running[0].1["name"], "cua_repl.js");
    assert_eq!(
        running[0].1["detail"],
        json!({"type":"unknown",
        "input":{"code":"await tab.goto('https://example.com')"},"output":null})
    );
    assert_eq!(codex_name(&json!({"type":"mcpToolCall","tool":"js"})), "js");
    assert_eq!(codex_name(&json!({"type":"mcpToolCall"})), "mcpToolCall");
    let failed = json!({"type":"mcpToolCall","error":{"message":"Access denied"}});
    assert_eq!(
        codex_error(&failed, true),
        json!({"message":"Access denied"})
    );
    assert_eq!(codex_error(&failed, false), Value::Null);
}

#[test]
fn claude_results_populate_typed_cards_and_unknown_tools_preserve_structured_results() {
    for (name, input, kind, field) in [
        ("Bash", json!({"command":"pwd"}), "shell", "output"),
        (
            "Read",
            json!({"file_path":"a.rs","offset":2}),
            "read",
            "content",
        ),
        ("Grep", json!({"pattern":"search"}), "search", "content"),
        (
            "WebFetch",
            json!({"url":"https://example.com","prompt":"Read"}),
            "fetch",
            "result",
        ),
        (
            "Agent",
            json!({"description":"Review","subagent_type":"reviewer"}),
            "sub_agent",
            "log",
        ),
    ] {
        let mut detail = claude(name, &input);
        assert_eq!(detail["type"], kind);
        claude_result(
            &mut detail,
            &json!([{"type":"text","text":"first"},{"type":"text","text":"second"}]),
        );
        assert_eq!(detail[field], "first\nsecond");
    }
    assert_eq!(
        claude(
            "Edit",
            &json!({"file_path":"a.rs","old_string":"old","new_string":"new"})
        )["oldString"],
        "old"
    );
    assert_eq!(
        claude("Write", &json!({"file_path":"a.rs","content":"new"}))["type"],
        "write"
    );
    assert_eq!(
        claude("ExitPlanMode", &json!({"plan":"Steps"}))["type"],
        "plan"
    );
    let mut unknown = claude("custom", &json!({"arg":1}));
    claude_result(&mut unknown, &json!({"result":true}));
    assert_eq!(unknown["output"], json!({"result":true}));
    let mut detail = claude("Read", &json!({"file_path":"a.rs"}));
    claude_result(&mut detail, &json!("界".repeat(PREVIEW_BYTES)));
    assert!(detail["content"].as_str().unwrap().len() <= PREVIEW_BYTES);
    assert!(
        detail["content"]
            .as_str()
            .unwrap()
            .contains("Output truncated")
    );
}
