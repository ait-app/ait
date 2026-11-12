use super::*;

fn call(name: &str, arguments: Value) -> Value {
    let mut call = json!({"id":"call-1","name":name});
    call["arguments"] = arguments;
    call
}

fn completed(name: &str, arguments: Value, result: &Value, text: Option<&str>) -> Value {
    item(
        &call(name, arguments),
        Some(Report { result, text }),
        Status::Completed,
    )
    .unwrap()
}

#[test]
fn bash_shows_a_shell_with_its_reported_exit_code_and_failure_line() {
    // Shapes recorded from OMP 18.18.8 (`echo boom; exit 3`).
    let result = json!({"content":[{"type":"text","text":"boom\n\n\nCommand exited with code 3"}],
        "details":{"exitCode":3,"timeoutSeconds":300},"isError":true});
    let text = "boom\n\n\nCommand exited with code 3";
    let arguments = json!({"i":"b","command":"echo boom; exit 3","cwd":"/work"});
    let running = item(&call("bash", arguments.clone()), None, Status::Running).unwrap();
    assert_eq!(
        running["detail"],
        json!({"type":"shell","command":"echo boom; exit 3","cwd":"/work"})
    );
    assert_eq!(running["status"], "running");
    assert!(running["error"].is_null());

    let report = Report {
        result: &result,
        text: Some(text),
    };
    assert!(failed("bash", report, true));
    let failure = item(&call("bash", arguments), Some(report), Status::Failed).unwrap();
    assert_eq!(failure["detail"]["output"], text);
    assert_eq!(failure["detail"]["exitCode"], 3);
    assert_eq!(failure["error"], "Command exited with code 3");
    assert_eq!(failure["callId"], "call-1");
    assert_eq!(failure["name"], "bash");
}

#[test]
fn reads_show_display_content_urls_fetch_and_devices_name_their_tool() {
    let result = json!({"content":[{"type":"text","text":"[a.txt#0097]\n1:one"}],
        "details":{"displayContent":{"text":"one","startLine":1}}});
    let file = completed(
        "read",
        json!({"i":"r","path":"a.txt"}),
        &result,
        Some("[a.txt#0097]\n1:one"),
    );
    assert_eq!(
        file["detail"],
        json!({"type":"read","filePath":"a.txt","content":"one"})
    );
    let page = completed(
        "read",
        json!({"path":"https://example.com/x"}),
        &json!({}),
        Some("URL"),
    );
    assert_eq!(
        page["detail"],
        json!({"type":"fetch","url":"https://example.com/x","result":"URL"})
    );
    let device = completed(
        "read",
        json!({"path":"xd://lsp/doc"}),
        &json!({}),
        Some("docs"),
    );
    assert_eq!(device["name"], "lsp");
    assert_eq!(
        device["detail"],
        json!({"type":"plain_text","label":"lsp","text":"docs"})
    );
}

#[test]
fn hashline_edits_name_their_file_then_show_the_reported_diff() {
    let arguments = json!({"i":"e","input":"[src/a.txt#0097]\nPUT 1.=1:\n+line 1\n"});
    let running = item(&call("edit", arguments.clone()), None, Status::Running).unwrap();
    assert_eq!(
        running["detail"],
        json!({"type":"edit","filePath":"src/a.txt"})
    );
    // OMP 18.18.8 reports the absolute path, whole-file texts and a line diff.
    let result = json!({"content":[{"type":"text","text":"ok"}],"details":{
        "diff":"-1|line one\n+1|line 1\n 2|line two","path":"/work/src/a.txt",
        "oldText":"line one\nline two\n","newText":"line 1\nline two\n"}});
    let edited = completed("edit", arguments, &result, Some("ok"));
    assert_eq!(
        edited["detail"],
        json!({"type":"edit","filePath":"/work/src/a.txt","oldString":"line one\nline two\n",
            "newString":"line 1\nline two\n","unifiedDiff":"-1|line one\n+1|line 1\n 2|line two"})
    );
    let legacy = item(
        &call(
            "edit",
            json!({"path":"b.rs","edits":[{"oldText":"a","newText":"b"}]}),
        ),
        None,
        Status::Running,
    )
    .unwrap();
    assert_eq!(
        legacy["detail"],
        json!({"type":"edit","filePath":"b.rs","oldString":"a","newString":"b"})
    );
    let unparsable = item(
        &call("edit", json!({"input":"no header"})),
        None,
        Status::Running,
    );
    assert_eq!(unparsable.unwrap()["detail"]["type"], "unknown");
}

#[test]
fn process_writes_stay_unknown_and_devices_show_the_executed_tool() {
    // O11: process control keeps its native input.
    let kill = json!({"path":"proc://collector/kill","content":null});
    let stopped = completed("write", kill.clone(), &json!({}), Some("Stopped"));
    assert_eq!(
        stopped["detail"],
        json!({"type":"unknown","input":kill,"output":"Stopped"})
    );
    let file = item(
        &call("write", json!({"path":"a.rs","content":"text"})),
        None,
        Status::Running,
    );
    assert_eq!(
        file.unwrap()["detail"],
        json!({"type":"write","filePath":"a.rs","content":"text"})
    );
    // OMP 18.18.8 `tools.xdev`: a device write reports the executed tool in `details.xdev`.
    let glob = json!({"path":"xd://glob","content":"{\"path\":\"*\"}"});
    let running = item(&call("write", glob.clone()), None, Status::Running).unwrap();
    assert_eq!(running["name"], "glob");
    assert_eq!(
        running["detail"],
        json!({"type":"plain_text","label":"glob"})
    );
    let result = json!({"content":[{"type":"text","text":"none"}],"details":{"xdev":{
        "tool":"glob","mode":"execute","args":{"path":"*"},"tier":"read","inner":{}}}});
    let executed = completed("write", glob, &result, Some("none"));
    assert_eq!(executed["name"], "glob");
    assert_eq!(
        executed["detail"],
        json!({"type":"unknown","input":{"path":"*"},"output":"none"})
    );
    let edit = json!({"path":"xd://ast_edit","content":"{\"op\":\"rename\",\"path\":\"a.rs\"}"});
    let opaque = completed("write", edit, &json!({}), Some("renamed"));
    assert_eq!(opaque["name"], "ast_edit");
    assert_eq!(
        opaque["detail"],
        json!({"type":"plain_text","label":"rename a.rs","text":"renamed"})
    );
}

#[test]
fn searches_carry_their_query_and_web_search_errors_fail() {
    let glob = completed(
        "glob",
        json!({"i":"g","path":"*.txt"}),
        &json!({}),
        Some("a.txt"),
    );
    assert_eq!(
        glob["detail"],
        json!({"type":"search","query":"*.txt","toolName":"glob","content":"a.txt"})
    );
    let grep = completed(
        "grep",
        json!({"pattern":"line"}),
        &json!({}),
        Some("1:line"),
    );
    assert_eq!(
        grep["detail"],
        json!({"type":"search","query":"line","toolName":"grep","content":"1:line"})
    );
    let find = completed("find", json!({"pattern":"*.rs"}), &json!({}), None);
    assert_eq!(
        find["detail"],
        json!({"type":"search","query":"*.rs","toolName":"search"})
    );
    let ls = completed("ls", json!({}), &json!({}), None);
    assert_eq!(ls["detail"], json!({"type":"search","query":"ls"}));

    let error = json!({"content":[],"details":{"error":"Exa: rate limited\nretry later"}});
    let report = Report {
        result: &error,
        text: None,
    };
    assert!(failed("web_search", report, false));
    assert!(!failed("grep", report, false));
    let search = item(
        &call("web_search", json!({"query":"rust"})),
        Some(report),
        Status::Failed,
    )
    .unwrap();
    assert_eq!(
        search["detail"],
        json!({"type":"search","query":"rust","toolName":"web_search"})
    );
    assert_eq!(search["error"], "Exa: rate limited");
}

#[test]
fn tasks_show_a_sub_agent_and_the_todo_tool_has_no_row() {
    let arguments = json!({"i":"delegate","context":"# Goal","tasks":[
        {"task":"# Target\nCHILD_JOB reply","solutionSpace":"one fix","name":"Prober"}]});
    let task = item(&call("task", arguments), None, Status::Running).unwrap();
    assert_eq!(
        task["detail"],
        json!({"type":"sub_agent","log":"","actions":[],"subAgentType":"Prober",
            "description":"delegate"})
    );
    let summarized = json!({"tasks":[{"task":"## goal\n\n  Ship it\nlater"}]});
    let result = json!({"details":{"sessionFile":"/s/child.jsonl"}});
    let detail = completed("task", summarized, &result, None)["detail"].clone();
    assert_eq!(detail["description"], "Ship it");
    // The stream names the child by its qualified identity, never by a file path.
    assert!(detail.get("childSessionId").is_none());
    assert!(detail.get("subAgentType").is_none());
    // OMP 18.18.8 names a background task's child in `progress` and `async.jobId`.
    for (details, expected) in [
        (
            json!({"results":[{"id":"done"}],"progress":[{"id":"later"}]}),
            Some("done"),
        ),
        (
            json!({"results":[],"progress":[{"id":"Prober"}],"async":{"jobId":"Job"}}),
            Some("Prober"),
        ),
        (json!({"results":[],"async":{"jobId":"Job"}}), Some("Job")),
        (json!({"results":[{"id":"../x"}]}), None),
        (json!({}), None),
    ] {
        assert_eq!(task_child(&details), expected, "{details}");
    }

    assert!(item(&call("todo", json!({"op":"init"})), None, Status::Running).is_none());
}

#[test]
fn opaque_and_assistant_tools_are_labelled_text_and_unknown_tools_keep_their_input() {
    // OMP 18.18.8 `wait` after a background bash job settled.
    let result = json!({"details":{"op":"wait","jobs":[{"id":"bg_1","type":"bash",
        "status":"completed","label":"sleep 1","resultText":"async-done"}]}});
    let wait = completed("wait", json!({"i":"w"}), &result, Some("## Completed (1)"));
    assert_eq!(
        wait["detail"],
        json!({"type":"plain_text","label":"bash completed: sleep 1","text":"async-done"})
    );
    let idle = completed(
        "wait",
        json!({}),
        &json!({}),
        Some(" Nothing to wait for\nmore "),
    );
    assert_eq!(idle["detail"]["label"], "Nothing to wait for");
    assert_eq!(
        completed("yield", json!({}), &json!({}), Some("done"))["detail"],
        json!({"type":"plain_text","label":"Submitted subagent result","text":"done"})
    );
    assert_eq!(
        completed("think", json!({"thoughts":"hmm"}), &json!({}), None)["detail"],
        json!({"type":"plain_text","label":"Thinking","text":"hmm"})
    );
    assert_eq!(
        completed("security_scan", json!({}), &json!({}), None)["detail"],
        json!({"type":"plain_text","label":"security scan"})
    );
    let asked = completed(
        "ask",
        json!({"i":"stack","html":"<form></form>"}),
        &json!({"details":{"results":[{"question":"DB?","selectedOptions":["A"],
            "customInput":"B"},{"question":"Auth?"}]}}),
        Some("User answers"),
    );
    assert_eq!(
        asked["detail"],
        json!({"type":"plain_text","label":"Question","text":"DB?\nA, B\n\nAuth?\nNo selection"})
    );
    let html = completed(
        "ask",
        json!({"html":"<form/>"}),
        &json!({}),
        Some("User answers:"),
    );
    assert_eq!(html["detail"]["text"], "User answers:");
    let vendor = completed("vendor_tool", json!({"value":42}), &json!({}), Some("out"));
    assert_eq!(
        vendor["detail"],
        json!({"type":"unknown","input":{"value":42},"output":"out"})
    );
    let large = json!({"path":"a.rs","content":"x".repeat(MAX_ARGUMENT_BYTES)});
    let bounded = item(&call("write", large), None, Status::Running).unwrap();
    assert_eq!(bounded["detail"]["type"], "unknown");
    assert!(bounded["detail"]["input"].as_str().unwrap().len() < MAX_ARGUMENT_BYTES);
}

#[test]
fn subagent_polls_of_the_same_jobs_share_a_stable_call_id() {
    let poll = |targets: Value| {
        item(
            &call("subagent", json!({"poll":targets})),
            None,
            Status::Running,
        )
        .unwrap()["callId"]
            .clone()
    };
    assert_eq!(poll(json!(["b", " a "])), "omp-poll:a,b");
    assert_eq!(poll(json!(["a", "b"])), "omp-poll:a,b");
    assert_eq!(poll(json!(["a", ""])), "call-1");
    assert_eq!(poll(json!([])), "call-1");
}

#[test]
fn failure_messages_prefer_the_reported_error_then_output_then_exit_code() {
    let message = |result: Value, text: Option<&str>| {
        failure_message(Report {
            result: &result,
            text,
        })
    };
    assert_eq!(
        message(
            json!({"details":{"error":{"message":"denied\nmore"}}}),
            None
        ),
        "denied"
    );
    assert_eq!(
        message(json!({}), Some("out\nProcess EXITED with code 12 now")),
        "Process EXITED with code 12"
    );
    assert_eq!(message(json!({}), Some("\n  first  \nsecond")), "first");
    assert_eq!(
        message(json!({}), Some(&"é".repeat(300))).chars().count(),
        240
    );
    assert_eq!(message(json!({"code":2}), None), "Tool exited with code 2");
    assert_eq!(message(json!({}), None), "Tool call failed");
    assert_eq!(exit_line("exited with code 1"), None);
    assert_eq!(exit_line("command exited with code x"), None);
}
