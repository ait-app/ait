use super::*;

fn command(raw: Value, actions: Value) -> Value {
    let mut native = json!({"type":"commandExecution","id":"command",
        "cwd":"/project","status":"completed","aggregatedOutput":"hello\n","exitCode":0});
    native["command"] = raw;
    native["commandActions"] = actions;
    native
}

fn entries(native: &Value) -> Vec<NativeItem> {
    timeline_items(
        native,
        "turn",
        "time",
        &crate::local::images::ImageStore::default(),
    )
    .unwrap()
}

#[test]
fn parsed_commands_take_precedence_over_paseo_shell_wrapper_cases() {
    for raw in [
        json!("/bin/zsh -lc 'echo hello'"),
        json!("/usr/bin/zsh -lc \"echo hello\""),
        json!(["/bin/zsh", "-lc", "echo hello"]),
        json!("/bin/bash -c 'echo hello'"),
        json!("pwsh.exe -NoProfile -Command \"echo hello\""),
        json!(["cmd.exe", "/c", "echo hello"]),
    ] {
        let native = command(raw, json!([{"type":"unknown","command":"echo hello"}]));
        let mapped = entries(&native);
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].key, "native:turn:command");
        assert_eq!(mapped[0].item["callId"], "command");
        assert_eq!(
            mapped[0].item["detail"],
            json!({"type":"shell",
            "command":"echo hello","cwd":"/project","output":"hello\n","exitCode":0})
        );
    }
}

#[test]
fn action_commands_preserve_quotes_and_meaningful_shell_invocations() {
    for parsed in [
        "printf '%s' \"a b\"",
        "/bin/zsh -lc 'echo nested'",
        "echo first && echo second",
    ] {
        let native = command(
            json!("outer wrapper"),
            json!([{"type":"unknown","command":parsed}]),
        );
        assert_eq!(entries(&native)[0].item["detail"]["command"], parsed);
    }
}

#[test]
fn compound_commands_expand_into_stable_typed_actions_in_source_order() {
    let native = command(
        json!("/bin/zsh -lc 'cat a.rs; ls src; rg TODO src; echo done'"),
        json!([
            {"type":"read","command":"cat a.rs","name":"a.rs","path":"/project/a.rs"},
            {"type":"listFiles","command":"ls src","path":"src"},
            {"type":"search","command":"rg TODO src","query":"TODO","path":"src"},
            {"type":"unknown","command":"echo done"}
        ]),
    );
    let mapped = entries(&native);
    assert_eq!(mapped.len(), 5);
    for (index, entry) in mapped[..4].iter().enumerate() {
        assert_eq!(entry.key, format!("native:turn:command:{index}"));
        assert_eq!(entry.item["callId"], format!("command:{index}"));
        assert_eq!(entry.item["status"], "completed");
    }
    assert_eq!(
        mapped[0].item["detail"],
        json!({"type":"read","filePath":"/project/a.rs","content":""})
    );
    assert_eq!(
        mapped[1].item["detail"],
        json!({"type":"search","toolName":"glob","query":"src","content":""})
    );
    assert_eq!(
        mapped[2].item["detail"],
        json!({"type":"search","toolName":"grep","query":"TODO","filePaths":["src"],"content":""})
    );
    assert_eq!(mapped[3].item["detail"]["command"], "echo done");
    assert_eq!(mapped[3].item["detail"]["output"], "");
    assert!(mapped[3].item["detail"].get("exitCode").is_none());
    assert_eq!(mapped[4].key, "native:turn:command:output");
    assert_eq!(mapped[4].item["detail"]["command"], native["command"]);
    assert_eq!(mapped[4].item["detail"]["output"], "hello\n");
    assert_eq!(mapped, entries(&native));
}

#[test]
fn absent_invalid_or_declined_actions_preserve_the_raw_command() {
    let raw = "/bin/zsh -lc 'echo hello'";
    for actions in [
        Value::Null,
        json!([]),
        json!({}),
        json!([{"type":"unknown"}]),
    ] {
        let mapped = entries(&command(json!(raw), actions));
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].item["detail"]["command"], raw);
    }
    let mut declined = command(
        json!(raw),
        json!([
            {"type":"unknown","command":"echo first"},
            {"type":"unknown","command":"echo second"}
        ]),
    );
    declined["status"] = json!("declined");
    let mapped = entries(&declined);
    assert_eq!(mapped.len(), 1);
    assert_eq!(mapped[0].item["detail"]["command"], raw);
    assert_eq!(mapped[0].item["status"], "failed");
    assert_eq!(mapped[0].item["error"], "Native tool failed");
}

#[test]
fn failed_commands_keep_their_parsed_actions_and_exit_status() {
    let mut native = command(
        json!("/bin/zsh -lc false"),
        json!([{"type":"unknown","command":"false"}]),
    );
    native["status"] = json!("failed");
    native["exitCode"] = json!(1);
    let mapped = entries(&native);
    assert_eq!(mapped[0].item["detail"]["command"], "false");
    assert_eq!(mapped[0].item["detail"]["exitCode"], 1);
    assert_eq!(mapped[0].item["status"], "failed");
}

#[test]
fn missing_action_metadata_uses_commands_and_does_not_drop_unknown_kinds() {
    let native = command(
        json!("wrapped"),
        json!([
            {"type":"listFiles","command":"ls","path":null},
            {"type":"search","command":"rg TODO","query":null,"path":null},
            {"type":"futureAction","command":"new command"}
        ]),
    );
    let mapped = entries(&native);
    assert_eq!(mapped[0].item["detail"]["query"], "ls");
    assert_eq!(mapped[1].item["detail"]["query"], "rg TODO");
    assert!(mapped[1].item["detail"].get("filePaths").is_none());
    assert_eq!(mapped[2].item["detail"]["command"], "new command");
}

#[test]
fn excessive_or_partially_malformed_actions_fall_back_without_partial_rows() {
    for actions in [
        json!([
            {"type":"unknown","command":"first"},
            {"type":"read","command":"cat missing-path"}
        ]),
        json!(vec![json!({"type":"unknown","command":"echo"}); 257]),
    ] {
        let mapped = entries(&command(json!("original command"), actions));
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].item["detail"]["command"], "original command");
    }
}

#[test]
fn batched_reads_keep_combined_stdout_out_of_individual_file_cards() {
    let mut native = command(
        json!("cat crates/relay/src/bridge.rs && cat apps/desktop/src/daemon/account-transport.ts"),
        json!([
            {"type":"read","command":"cat crates/relay/src/bridge.rs",
                "path":"crates/relay/src/bridge.rs"},
            {"type":"read","command":"cat apps/desktop/src/daemon/account-transport.ts",
                "path":"apps/desktop/src/daemon/account-transport.ts"}
        ]),
    );
    native["aggregatedOutput"] =
        json!("use std::sync::Arc;\nimport type { WebContents } from \"electron\";\n");

    let mapped = entries(&native);

    assert_eq!(mapped.len(), 3);
    for (index, entry) in mapped[..2].iter().enumerate() {
        assert_eq!(entry.item["detail"]["type"], "read");
        assert_eq!(entry.item["detail"]["content"], "");
        assert_eq!(entry.key, format!("native:turn:command:{index}"));
    }
    assert_eq!(mapped[2].key, "native:turn:command:output");
    assert_eq!(mapped[2].item["detail"]["type"], "shell");
    assert_eq!(mapped[2].item["detail"]["command"], native["command"]);
    assert_eq!(
        mapped[2].item["detail"]["output"],
        native["aggregatedOutput"]
    );
    assert_eq!(mapped[2].item["detail"]["exitCode"], 0);
}

#[test]
fn single_read_keeps_its_own_output_and_call_identity() {
    let native = command(
        json!("cat account-transport.ts"),
        json!([{"type":"read","command":"cat account-transport.ts",
            "path":"account-transport.ts"}]),
    );

    let mapped = entries(&native);

    assert_eq!(mapped.len(), 1);
    assert_eq!(mapped[0].key, "native:turn:command");
    assert_eq!(mapped[0].item["detail"]["content"], "hello\n");
}

#[test]
fn compound_failure_exit_status_belongs_only_to_the_combined_command() {
    let mut native = command(
        json!("echo first && false && echo skipped"),
        json!([
            {"type":"unknown","command":"echo first"},
            {"type":"unknown","command":"false"},
            {"type":"unknown","command":"echo skipped"}
        ]),
    );
    native["status"] = json!("failed");
    native["exitCode"] = json!(1);

    let mapped = entries(&native);

    assert_eq!(mapped.len(), 4);
    for action in &mapped[..3] {
        assert_eq!(action.item["detail"]["output"], "");
        assert!(action.item["detail"].get("exitCode").is_none());
    }
    assert_eq!(mapped[3].item["status"], "failed");
    assert_eq!(mapped[3].item["detail"]["exitCode"], 1);
    assert_eq!(mapped[3].item["detail"]["output"], "hello\n");
}
