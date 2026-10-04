use serde_json::{Map, Value, json};

use super::{
    IMAGE_PLACEHOLDER, NATIVE_WITHDRAWAL, Translator, effect_of, final_text, is_withdrawal,
    normalize_id, usage, uuid_of,
};
use crate::event::{AskKind, Body, Effect, Level, Origin, TodoState, ToolKind, ToolState};
use crate::wire::{Answer, Person};

fn owner() -> Person {
    Person {
        id: "github:900001".to_owned(),
        login: Some("sandbox-owner".to_owned()),
    }
}

fn translator() -> Translator {
    Translator::new(Some(owner()))
}

fn answer(option: &str, answers: Option<Value>, note: Option<&str>) -> Answer {
    Answer {
        run_id: "r_1".to_owned(),
        by: Person {
            id: "github:2".to_owned(),
            login: None,
        },
        ask_id: "req".to_owned(),
        option_id: option.to_owned(),
        answers: answers.and_then(|value| value.as_object().cloned()),
        note: note.map(str::to_owned),
    }
}

#[test]
fn ids_normalize_and_become_lowercase_uuids() {
    assert_eq!(normalize_id("0123ABCD-ef01"), "0123abcdef01");
    assert_eq!(
        uuid_of("0123456789ABCDEF0123456789abcdef"),
        "01234567-89ab-cdef-0123-456789abcdef"
    );
    let derived = uuid_of("r_not-hex");
    assert_eq!(derived.len(), 36);
    assert_eq!(derived, uuid_of("r_not-hex"));
}

#[test]
fn assistant_deltas_and_suffixes_append_per_message() {
    let mut translator = translator();
    let progress = translator.item(
        "claude",
        &json!({"type": "assistant_message", "messageId": "msg_1:0", "text": "Hello "}),
    );
    let suffix = translator.item(
        "claude",
        &json!({"type": "assistant_message", "messageId": "msg_1:0", "text": "world"}),
    );
    let empty = translator.item(
        "claude",
        &json!({"type": "assistant_message", "messageId": "msg_1:0", "text": ""}),
    );
    assert_eq!(
        progress,
        vec![Body::Text {
            mid: "msg_1:0".to_owned(),
            text: "Hello ".to_owned()
        }]
    );
    assert_eq!(
        suffix,
        vec![Body::Text {
            mid: "msg_1:0".to_owned(),
            text: "world".to_owned()
        }]
    );
    assert!(empty.is_empty(), "an empty completed suffix sends nothing");
}

#[test]
fn appending_a_merged_entry_duplicates_streamed_text() {
    // Guard: the merged display projection must never be appended to a live log.
    let mut translator = translator();
    let mut streamed = String::new();
    for delta in ["Hel", "lo"] {
        for body in translator.item(
            "claude",
            &json!({"type": "assistant_message", "messageId": "m", "text": delta}),
        ) {
            if let Body::Text { text, .. } = body {
                streamed.push_str(&text);
            }
        }
    }
    let merged = translator.item(
        "claude",
        &json!({"type": "assistant_message", "messageId": "m", "text": "Hello"}),
    );
    if let Some(Body::Text { text, .. }) = merged.first() {
        streamed.push_str(text);
    }
    assert_eq!(streamed, "HelloHello");
}

#[test]
fn images_never_expose_local_paths_or_remote_urls() {
    let mut translator = translator();
    for source in [
        "![Image](/var/folders/x/ait-provider-images-1/abc.png)",
        "![Image](https://tracker.example.com/pixel.png)",
    ] {
        let bodies = translator.item(
            "claude",
            &json!({"type": "assistant_message", "messageId": "m:image:0", "text": source}),
        );
        assert_eq!(
            bodies,
            vec![Body::Text {
                mid: "m:image:0".to_owned(),
                text: IMAGE_PLACEHOLDER.to_owned()
            }]
        );
    }
    let codex_image = translator.item(
        "codex",
        &json!({"type": "tool_call", "callId": "i1", "name": "imageView", "status": "completed",
                "detail": {"type": "plain_text", "label": "Image", "text": "/Users/me/private/shot.png"}}),
    );
    let text = serde_json::to_string(&codex_image).expect("serializes");
    assert!(!text.contains("/Users/me"), "{text}");
}

#[test]
fn reasoning_has_no_message_id() {
    let mut translator = translator();
    assert_eq!(
        translator.item("codex", &json!({"type": "reasoning", "text": "think"})),
        vec![Body::Reasoning {
            mid: None,
            text: "think".to_owned()
        }]
    );
}

#[test]
fn shell_tools_map_states_titles_and_fields() {
    let mut translator = translator();
    let running = translator.item(
        "claude",
        &json!({"type": "tool_call", "callId": "toolu_1", "name": "Bash", "status": "running", "error": null,
                "detail": {"type": "shell", "command": "ls -la\necho done", "description": "List"}}),
    );
    let Body::Tool {
        state,
        kind,
        title,
        input,
        truncated,
        ..
    } = &running[0]
    else {
        panic!("expected a tool");
    };
    assert_eq!(*state, ToolState::Running);
    assert_eq!(*kind, Some(ToolKind::Shell));
    assert_eq!(title.as_deref(), Some("ls -la"));
    assert_eq!(input.as_deref(), Some("ls -la\necho done"));
    assert_eq!(*truncated, Some(false));
    let done = translator.item(
        "codex",
        &json!({"type": "tool_call", "callId": "item_7", "name": "commandExecution", "status": "completed",
                "detail": {"type": "shell", "command": "ls", "output": "a\n", "cwd": "/repo", "exitCode": 0}}),
    );
    let Body::Tool {
        state,
        input,
        output,
        ..
    } = &done[0]
    else {
        panic!("expected a tool");
    };
    assert_eq!(*state, ToolState::Ok);
    assert_eq!(input.as_deref(), Some("ls\n(cwd: /repo)"));
    assert_eq!(output.as_deref(), Some("a\n\nexit code: 0"));
}

#[test]
fn codex_running_tails_leave_truncation_unknown() {
    let mut translator = translator();
    let tail = translator.item(
        "codex",
        &json!({"type": "tool_call", "callId": "c", "name": "commandExecution", "status": "running",
                "detail": {"type": "shell", "command": "make", "output": "tail"}}),
    );
    assert!(matches!(
        tail[0],
        Body::Tool {
            truncated: None,
            ..
        }
    ));
}

#[test]
fn oversized_and_marked_outputs_are_truncated() {
    let mut translator = translator();
    let big = "x".repeat(20_000);
    let bodies = translator.item(
        "claude",
        &json!({"type": "tool_call", "callId": "t", "name": "Read", "status": "completed",
                "detail": {"type": "read", "filePath": "/a", "content": big}}),
    );
    let Body::Tool {
        output, truncated, ..
    } = &bodies[0]
    else {
        panic!("expected a tool");
    };
    assert!(output.as_deref().is_some_and(|o| o.len() <= 16 * 1024));
    assert_eq!(*truncated, Some(true));
    let marked = translator.item(
        "claude",
        &json!({"type": "tool_call", "callId": "u", "name": "Bash", "status": "completed",
                "detail": {"type": "shell", "command": "cat", "output": "abc\n[Output truncated; full output remains in the native transcript.]"}}),
    );
    assert!(matches!(
        marked[0],
        Body::Tool {
            truncated: Some(true),
            ..
        }
    ));
}

#[test]
fn every_detail_type_has_a_presentation() {
    let mut translator = translator();
    let cases = [
        (
            json!({"type": "edit", "filePath": "/f", "oldString": "a", "newString": "b"}),
            "Edit",
            ToolKind::Edit,
        ),
        (
            json!({"type": "write", "filePath": "/f", "content": "c"}),
            "Write",
            ToolKind::Write,
        ),
        (
            json!({"type": "search", "toolName": "grep", "query": "q", "content": "r"}),
            "Grep",
            ToolKind::Search,
        ),
        (
            json!({"type": "fetch", "url": "https://x", "result": "r"}),
            "WebFetch",
            ToolKind::Fetch,
        ),
        (
            json!({"type": "sub_agent", "description": "d", "prompt": "p", "log": ""}),
            "Task",
            ToolKind::Agent,
        ),
        (json!({"type": "plan", "text": "t"}), "plan", ToolKind::Plan),
        (
            json!({"type": "unknown", "input": {"path": "x"}, "output": "ok"}),
            "mcp__bonsai_run__read_note",
            ToolKind::Mcp,
        ),
        (
            json!({"type": "unknown", "input": {"a": 1}}),
            "Custom",
            ToolKind::Other,
        ),
        (json!({"type": "brand_new"}), "Thing", ToolKind::Other),
    ];
    for (detail, name, expected) in cases {
        let bodies = translator.item(
            "claude",
            &json!({"type": "tool_call", "callId": "c", "name": name, "status": "completed", "detail": detail}),
        );
        let Body::Tool { kind, title, .. } = &bodies[0] else {
            panic!("expected a tool for {name}");
        };
        assert_eq!(*kind, Some(expected), "{name}");
        assert!(title.is_some(), "{name}");
    }
    let mcp = translator.item(
        "claude",
        &json!({"type": "tool_call", "callId": "c", "name": "mcp__bonsai_run__read_note", "status": "completed",
                "detail": {"type": "unknown", "input": {"path": "x"}, "output": "ok"}}),
    );
    assert!(matches!(&mcp[0], Body::Tool { title: Some(t), .. } if t == "bonsai_run · read_note"));
    let edit = translator.item(
        "claude",
        &json!({"type": "tool_call", "callId": "e", "name": "Edit", "status": "completed",
                "detail": {"type": "edit", "filePath": "/f", "oldString": "old", "newString": "new"}}),
    );
    assert!(
        matches!(&edit[0], Body::Tool { input: Some(i), .. } if i == "--- /f\n+++ /f\n-old\n+new\n")
    );
}

#[test]
fn claude_task_tools_are_hidden_by_exact_name_but_sub_agents_show() {
    let mut translator = translator();
    for hidden in [
        "TodoWrite",
        "TaskCreate",
        "TaskUpdate",
        "TaskList",
        "ExitPlanMode",
    ] {
        assert!(
            translator
                .item("claude", &json!({"type": "tool_call", "callId": "c", "name": hidden, "status": "running", "detail": {"type": "unknown"}}))
                .is_empty(),
            "{hidden}"
        );
    }
    let task = translator.item(
        "claude",
        &json!({"type": "tool_call", "callId": "c", "name": "Task", "status": "running",
                "detail": {"type": "sub_agent", "description": "explore", "log": ""}}),
    );
    assert!(matches!(
        &task[0],
        Body::Tool {
            kind: Some(ToolKind::Agent),
            ..
        }
    ));
}

#[test]
fn todo_states_map_and_unknown_states_stay_pending() {
    let mut translator = translator();
    let bodies = translator.item(
        "claude",
        &json!({"type": "todo", "items": [
            {"id": "1", "text": "a", "status": "pending"},
            {"id": "2", "text": "b", "status": "in_progress"},
            {"id": "3", "text": "c", "status": "completed"},
            {"id": "4", "text": "d", "status": "weird"}]}),
    );
    let Body::Todo { items } = &bodies[0] else {
        panic!("expected todo");
    };
    let states: Vec<TodoState> = items.iter().map(|item| item.state).collect();
    assert_eq!(
        states,
        [
            TodoState::Pending,
            TodoState::Active,
            TodoState::Done,
            TodoState::Pending
        ]
    );
}

#[test]
fn only_own_input_echoes_are_dropped_and_local_input_is_the_owners() {
    let mut translator = translator();
    translator.sent("01234567-89ab-cdef-0123-456789abcdef");
    let echo = translator.item(
        "claude",
        &json!({"type": "user_message", "messageId": "01234567-89ab-cdef-0123-456789abcdef",
                "clientMessageId": "0123456789ABCDEF0123456789abcdef", "text": "<task>…</task>"}),
    );
    assert!(echo.is_empty());
    let local = translator.item(
        "claude",
        &json!({"type": "user_message", "messageId": "aaaa", "clientMessageId": "aaaa", "text": "typed in AIT"}),
    );
    assert_eq!(
        local,
        vec![Body::Input {
            id: "aaaa".to_owned(),
            text: "typed in AIT".to_owned(),
            by: "github:900001".to_owned(),
            login: Some("sandbox-owner".to_owned()),
            origin: Origin::User,
        }]
    );
}

#[test]
fn notifications_compaction_and_unknown_items_become_notices() {
    let mut translator = translator();
    assert!(matches!(
        translator.item(
            "claude",
            &json!({"type": "notification", "level": "warning", "message": "m"})
        )[0],
        Body::Notice {
            level: Level::Warning,
            ..
        }
    ));
    assert_eq!(
        translator
            .item(
                "claude",
                &json!({"type": "compaction", "status": "completed"})
            )
            .len(),
        1
    );
    assert!(
        translator
            .item(
                "claude",
                &json!({"type": "compaction", "status": "started"})
            )
            .is_empty()
    );
    assert!(matches!(
        translator.item("claude", &json!({"type": "plugin", "id": "x"}))[0],
        Body::Notice {
            level: Level::Info,
            ..
        }
    ));
    assert!(translator.item("claude", &json!({"no": "type"})).is_empty());
}

#[test]
fn usage_maps_token_counts_and_context() {
    assert_eq!(
        usage(
            &json!({"inputTokens": 1200, "outputTokens": 300, "cachedInputTokens": 9,
                      "contextWindowMaxTokens": 200_000, "contextWindowUsedTokens": 15_000})
        ),
        Some(Body::Usage {
            input: Some(1200),
            output: Some(300),
            context: Some(crate::event::ContextUsage {
                used: 15_000,
                max: 200_000
            }),
        })
    );
    assert_eq!(usage(&Value::Null), None);
    assert_eq!(usage(&json!({})), None, "an empty snapshot says nothing");
    assert_eq!(usage(&json!({"totalCostUsd": 0.1})), None);
}

fn claude_tool_request() -> Value {
    json!({"id": "11111111-2222-4333-8444-555555555555", "provider": "claude", "name": "Bash", "kind": "tool",
           "input": {"command": "touch /tmp/bonsai-sandbox.txt\nmore"},
           "suggestions": [
               {"type": "addRules", "rules": [{"toolName": "Bash", "ruleContent": "touch:*"}], "behavior": "allow", "destination": "localSettings"},
               {"type": "addRules", "rules": [{"toolName": "Bash", "ruleContent": "touch:*"}], "behavior": "allow", "destination": "session"}],
           "actions": [
               {"id": "allow", "label": "Allow once", "behavior": "allow", "variant": "primary"},
               {"id": "deny", "label": "Deny", "behavior": "deny", "variant": "secondary"},
               {"id": "allow-update-0", "label": "Allow rule in localSettings", "behavior": "allow"},
               {"id": "allow-update-1", "label": "Allow rule in session", "behavior": "allow"}]})
}

#[test]
fn tool_requests_offer_only_one_time_or_session_grants() {
    let spec = translator().ask(&claude_tool_request()).expect("an ask");
    assert_eq!(spec.kind, AskKind::Tool);
    assert_eq!(spec.title, "Bash: touch /tmp/bonsai-sandbox.txt");
    assert_eq!(
        spec.detail.as_deref(),
        Some("touch /tmp/bonsai-sandbox.txt\nmore")
    );
    let ids: Vec<&str> = spec
        .options
        .iter()
        .map(|option| option.id.as_str())
        .collect();
    assert_eq!(ids, ["allow", "deny", "allow-update-1"]);
    assert!(spec.options[2].label.contains("Bash(touch:*)"));
}

#[test]
fn every_ask_has_a_nonempty_title_and_tool_asks_have_detail() {
    let translator = translator();
    for request in [
        json!({"id": "a1", "provider": "claude", "name": "mcp__x__y", "kind": "tool", "input": {}, "actions": []}),
        json!({"id": "a2", "provider": "codex", "name": "commandExecution", "kind": "tool", "input": {"command": ["git", "status"]}}),
        json!({"id": "a3", "provider": "codex", "name": "fileChange", "kind": "tool", "input": {"itemId": "i9"}}),
        json!({"id": "a4", "provider": "codex", "name": "McpElicitation", "kind": "tool", "title": "Allow?", "input": {}}),
        json!({"id": "a5", "provider": "claude", "name": "", "kind": "tool"}),
        json!({"id": "plan-approval:abc", "provider": "codex", "name": "CodexPlanApproval", "kind": "plan", "input": {"plan": "step"},
               "actions": [{"id": "dismiss", "behavior": "deny"}, {"id": "implement", "behavior": "allow"}]}),
    ] {
        let spec = translator.ask(&request).expect("an ask");
        assert!(!spec.title.is_empty(), "{request}");
        assert!(spec.title.len() <= 200);
        if spec.kind == AskKind::Tool {
            assert!(
                spec.detail
                    .as_deref()
                    .is_some_and(|detail| !detail.is_empty()),
                "{request}"
            );
        }
        assert!(spec.options.iter().any(|o| o.effect == Effect::Allow));
        assert!(spec.options.iter().any(|o| o.effect == Effect::Deny));
    }
    assert!(translator.ask(&json!({"id": "", "name": "Bash"})).is_none());
}

#[test]
fn request_ids_outside_the_contract_pattern_are_remapped_and_answered_natively() {
    // Arrange
    let translator = translator();

    // Act
    let spec = translator
        .ask(&json!({"id": "bad id!", "name": "Bash", "input": {"command": "ls"}}))
        .expect("an ask");
    let fitting = translator
        .ask(&json!({"id": "toolu_01", "name": "Bash", "input": {"command": "ls"}}))
        .expect("an ask");

    // Assert
    assert!(
        spec.id.starts_with("x-") && spec.id.len() == 34,
        "{}",
        spec.id
    );
    assert_eq!(spec.id, super::ask_id("bad id!"), "stable across events");
    assert_eq!(spec.native_id(), "bad id!");
    assert_eq!(fitting.id, "toolu_01");
    assert_eq!(fitting.native_id, None);
    let stored: super::AskSpec =
        serde_json::from_str(&serde_json::to_string(&spec).expect("json")).expect("round trip");
    assert_eq!(stored.native_id(), "bad id!");
}

#[test]
fn empty_native_ids_never_reach_the_page() {
    let mut translator = translator();
    let text = translator.item(
        "claude",
        &json!({"type": "assistant_message", "messageId": "", "text": "hi"}),
    );
    assert_eq!(
        text,
        [Body::Text {
            mid: "assistant".to_owned(),
            text: "hi".to_owned()
        }]
    );
    let tool = translator.item(
        "claude",
        &json!({"type": "tool_call", "callId": "", "name": "Bash", "status": "running"}),
    );
    assert!(
        tool.is_empty(),
        "a tool call without an ID cannot be followed"
    );
    let input = translator.item(
        "claude",
        &json!({"type": "user_message", "messageId": "", "text": "typed"}),
    );
    match &input[..] {
        [Body::Input { id, .. }] => assert!(id.starts_with("u-"), "{id}"),
        other => panic!("expected an input, got {other:?}"),
    }
    let question = translator
        .ask(
            &json!({"id": "q1", "provider": "codex", "name": "requestUserInput", "kind": "question",
                     "input": {"questions": [{"id": "", "question": "Why?"}]}}),
        )
        .expect("an ask");
    assert_eq!(question.questions.expect("questions")[0].key, "q0");
}

#[test]
fn unknown_tool_details_lose_path_like_fields() {
    let mut translator = translator();
    let bodies = translator.item(
        "codex",
        &json!({"type": "tool_call", "callId": "c1", "name": "mystery", "status": "completed",
                "detail": {"type": "novel", "input": {"cwd": "/Users/me/repo", "query": "x",
                           "nested": {"filePath": "/Users/me/a.rs", "keep": 1}, "working_directory": "/tmp/w",
                           "paths": ["/Users/me/b"], "rootDir": "/Users/me/c", "outputDir": "/Users/me/d",
                           "folder": "/Users/me/e", "grantRoot": "/Users/me/f", "directories": ["/tmp/g"]},
                           "output": {"root": "/repo", "result": "ok"}}}),
    );
    let [Body::Tool { input, output, .. }] = &bodies[..] else {
        panic!("expected a tool, got {bodies:?}");
    };
    let input = input.as_deref().expect("input");
    let output = output.as_deref().expect("output");
    assert!(
        !input.contains("/Users/me") && !input.contains("/tmp/w"),
        "{input}"
    );
    assert!(
        input.contains("\"query\"") && input.contains("\"keep\""),
        "{input}"
    );
    assert!(
        !output.contains("/repo") && output.contains("ok"),
        "{output}"
    );
}

#[test]
fn codex_drops_persistent_grants_and_grant_root_requires_session_scope() {
    let translator = translator();
    let spec = translator
        .ask(&json!({"id": "c1", "provider": "codex", "name": "commandExecution", "kind": "tool",
                     "input": {"command": "npm test"},
                     "actions": [{"id": "allow", "behavior": "allow"}, {"id": "deny", "behavior": "deny"},
                                 {"id": "allow-session", "behavior": "allow"}, {"id": "allow-prefix", "behavior": "allow"},
                                 {"id": "network-0", "behavior": "allow"}]}))
        .expect("ask");
    let ids: Vec<&str> = spec.options.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, ["allow", "deny", "allow-session"]);
    let rooted = translator
        .ask(&json!({"id": "c2", "provider": "codex", "name": "fileChange", "kind": "tool",
                     "input": {"itemId": "x", "grantRoot": "/repo"},
                     "actions": [{"id": "allow", "behavior": "allow"}, {"id": "deny", "behavior": "deny"},
                                 {"id": "allow-session", "behavior": "allow"}]}))
        .expect("ask");
    let ids: Vec<&str> = rooted.options.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, ["deny", "allow-session"]);
    assert_eq!(
        rooted.options[1].label, "本次会话里都允许写入 /repo",
        "the grant is named"
    );
    assert!(
        rooted
            .detail
            .as_deref()
            .is_some_and(|detail| detail.starts_with("grantRoot: /repo\n")),
        "{:?}",
        rooted.detail
    );
    let session_less = translator
        .ask(&json!({"id": "c3", "provider": "codex", "name": "fileChange", "kind": "tool",
                     "input": {"itemId": "x", "grantRoot": "/repo"},
                     "actions": [{"id": "allow", "behavior": "allow"}, {"id": "deny", "behavior": "deny"}]}))
        .expect("ask");
    let ids: Vec<&str> = session_less.options.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, ["deny"], "no synthetic allow AIT would refuse");
}

#[test]
fn claude_questions_round_trip_to_native_answers() {
    let spec = translator()
        .ask(&json!({"id": "q", "provider": "claude", "name": "AskUserQuestion", "kind": "question",
                     "input": {"questions": [
                         {"question": "Which color?", "header": "Color", "multiSelect": true, "allowOther": true,
                          "options": [{"label": "red", "description": "warm"}, {"label": "blue"}]}]},
                     "actions": [{"id": "allow", "behavior": "allow"}, {"id": "deny", "behavior": "deny"}]}))
        .expect("ask");
    assert_eq!(spec.kind, AskKind::Question);
    assert_eq!(spec.title, "Color");
    let questions = spec.questions.clone().expect("questions");
    assert_eq!(questions[0].key, "q0");
    assert!(questions[0].multi && questions[0].other);
    assert_eq!(spec.options.len(), 2);
    let (response, effect) = spec
        .response(&answer(
            "allow",
            Some(json!({"q0": ["red", "green"]})),
            None,
        ))
        .expect("valid answer");
    assert_eq!(effect, Effect::Allow);
    assert_eq!(
        response,
        json!({"behavior": "allow", "selectedActionId": "allow", "updatedInput": {"answers": {"Which color?": "red, green"}}})
    );
    assert!(
        spec.response(&answer("allow", Some(json!({})), None))
            .is_err()
    );
    assert!(
        spec.response(&answer("allow", Some(json!({"q0": [""]})), None))
            .is_err()
    );
    assert!(
        spec.response(&answer(
            "allow",
            Some(json!({"q0": ["z".repeat(5000)]})),
            None
        ))
        .is_err()
    );
}

#[test]
fn codex_questions_answer_by_id_with_arrays() {
    let spec = translator()
        .ask(&json!({"id": "q2", "provider": "codex", "name": "request_user_input", "kind": "question",
                     "input": {"questions": [{"id": "env", "header": "Env", "question": "Which?", "isOther": true,
                                              "options": [{"label": "prod"}]}]}}))
        .expect("ask");
    let (response, _) = spec
        .response(&answer("allow", Some(json!({"env": ["prod"]})), None))
        .expect("valid");
    assert_eq!(
        response,
        json!({"behavior": "allow", "updatedInput": {"answers": {"env": ["prod"]}}})
    );
}

#[test]
fn deny_and_allow_responses_echo_only_native_actions() {
    let spec = translator().ask(&claude_tool_request()).expect("ask");
    let (deny, effect) = spec
        .response(&answer("deny", None, Some("not now")))
        .expect("deny");
    assert_eq!(effect, Effect::Deny);
    assert_eq!(
        deny,
        json!({"behavior": "deny", "selectedActionId": "deny", "message": "not now", "interrupt": false})
    );
    let (session, _) = spec
        .response(&answer("allow-update-1", None, None))
        .expect("allow");
    assert_eq!(
        session,
        json!({"behavior": "allow", "selectedActionId": "allow-update-1"})
    );
    assert!(
        spec.response(&answer("allow-update-0", None, None))
            .is_err()
    );
    let mut bare = spec.clone();
    bare.native_actions.clear();
    let (plain, _) = bare.response(&answer("allow", None, None)).expect("allow");
    assert_eq!(plain, json!({"behavior": "allow"}));
    assert!(serde_json::to_string(&deny).is_ok_and(|text| !text.contains("updatedPermissions")));
}

#[test]
fn the_native_withdrawal_sentence_is_pinned() {
    assert_eq!(NATIVE_WITHDRAWAL, "Resolved by native provider");
    assert!(is_withdrawal(
        &json!({"behavior": "deny", "message": "Resolved by native provider"})
    ));
    assert!(!is_withdrawal(
        &json!({"behavior": "deny", "message": "Denied by user"})
    ));
    assert_eq!(effect_of(&json!({"behavior": "allow"})), Effect::Allow);
    assert_eq!(effect_of(&json!({"behavior": "deny"})), Effect::Deny);
    assert_eq!(effect_of(&Value::Object(Map::new())), Effect::Deny);
}

#[test]
fn final_text_is_cut_on_a_code_point() {
    assert_eq!(final_text(""), None);
    let text = "长".repeat(2000);
    let cut = final_text(&text).expect("text");
    assert!(cut.len() <= 4096);
    assert!(text.starts_with(&cut));
}

#[test]
fn codex_mcp_rows_named_server_dot_tool_become_mcp_tools() {
    // Arrange: the row AIT 0.0.16 publishes for a Codex MCP call (`tool_detail::codex_tools`).
    let mut translator = translator();
    let row = json!({"type": "tool_call", "callId": "call", "name": "bonsai_run.read_note",
        "status": "completed", "error": null, "detail": {"type": "unknown",
        "input": {"space": "sandbox", "path": "1_Projects/Sandbox/Sandbox.md"},
        "output": {"content": [{"type": "text", "text": "ok"}]}}});

    // Act
    let bodies = translator.item("codex", &row);

    // Assert
    let Body::Tool {
        kind, title, input, ..
    } = &bodies[0]
    else {
        panic!("expected a tool");
    };
    assert_eq!(*kind, Some(ToolKind::Mcp));
    assert_eq!(title.as_deref(), Some("bonsai_run · read_note"));
    assert!(
        input
            .as_deref()
            .is_some_and(|input| input.contains("1_Projects/Sandbox/Sandbox.md")),
        "a Bonsai note path is an MCP argument, not a local path"
    );
    for name in [".read_note", "bonsai_run.", "commandExecution"] {
        let bodies = translator.item(
            "codex",
            &json!({"type": "tool_call", "callId": name, "name": name, "status": "completed",
                    "detail": {"type": "unknown", "input": {"a": 1}}}),
        );
        assert!(
            matches!(
                &bodies[0],
                Body::Tool {
                    kind: Some(ToolKind::Other),
                    ..
                }
            ),
            "{name}"
        );
    }
}

#[test]
fn provider_error_rows_become_error_notices() {
    // Arrange
    let mut translator = translator();
    let capacity = "Selected model is at capacity. Please try a different model.";

    // Act
    let reported = translator.item("codex", &json!({"type": "error", "message": capacity}));
    let bare = translator.item("codex", &json!({"type": "error"}));

    // Assert
    assert!(matches!(&reported[0], Body::Notice { level: Level::Error, text } if text == capacity));
    assert!(
        matches!(&bare[0], Body::Notice { level: Level::Error, text } if text == "执行端报告了一个错误")
    );
}
