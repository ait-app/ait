use super::*;

fn progress(stream: &mut Stream, method: &str, params: &Value) -> Vec<NativeItem> {
    let first = stream.progress(method, params).unwrap();
    first
        .into_iter()
        .chain(stream.events.drain(..))
        .map(|event| item(Some(event)))
        .collect()
}

#[test]
fn live_actions_keep_the_same_keys_and_details_as_completed_history() {
    let mut stream = Stream::default();
    let mut native = json!({"type":"commandExecution","id":"tool","status":"inProgress",
    "command":"/bin/zsh -lc 'cat a.rs; rg TODO src; echo done'","cwd":"/project",
    "commandActions":[
        {"type":"read","command":"cat a.rs","path":"/project/a.rs"},
        {"type":"search","command":"rg TODO src","query":"TODO","path":"src"},
        {"type":"unknown","command":"echo done"}
    ]});
    let started = json!({"turnId":"turn","item":native});
    let running = progress(&mut stream, "item/started", &started);
    assert_eq!(running.len(), 4);
    assert!(
        running
            .iter()
            .all(|entry| entry.item["status"] == "running")
    );
    assert_eq!(
        progress(&mut stream, "item/started", &started),
        [] as [NativeItem; 0]
    );
    for delta in ["hello", " world"] {
        let updates = progress(
            &mut stream,
            "item/commandExecution/outputDelta",
            &params("tool", delta),
        );
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].key, running[3].key);
    }
    let updates = progress(
        &mut stream,
        "item/commandExecution/outputDelta",
        &params("tool", "!"),
    );
    native["status"] = json!("completed");
    native["aggregatedOutput"] = json!("hello world!");
    let completed = discovery::timeline_items(
        &native,
        "turn",
        "time",
        &crate::local::images::ImageStore::default(),
    )
    .unwrap();
    assert_eq!(completed.len(), running.len());
    for (live, finished) in running.iter().zip(&completed) {
        assert_eq!(live.key, finished.key);
        assert_eq!(finished.item["status"], "completed");
    }
    assert_eq!(updates[0].item["detail"], completed[3].item["detail"]);
    assert_eq!(completed[0].item["detail"]["content"], "");
    assert_eq!(completed[1].item["detail"]["content"], "");
    assert_eq!(completed[2].item["detail"]["output"], "");
    assert!(stream.complete(&native).unwrap());
    assert!(!stream.complete(&native).unwrap());
    assert_eq!(
        progress(
            &mut stream,
            "item/commandExecution/outputDelta",
            &params("tool", "late")
        ),
        [] as [NativeItem; 0]
    );
}

#[test]
fn only_the_combined_command_receives_the_bounded_unicode_output_tail() {
    let mut stream = Stream::default();
    progress(
        &mut stream,
        "item/started",
        &json!({"turnId":"turn","item":{
        "type":"commandExecution","id":"tool","command":"wrapped",
        "commandActions":[
            {"type":"listFiles","command":"ls","path":null},
            {"type":"unknown","command":"echo done"}
        ]}}),
    );
    let output = "界".repeat(MAX_OUTPUT);
    let updates = progress(
        &mut stream,
        "item/commandExecution/outputDelta",
        &params("tool", &output),
    );
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].key, "native:turn:tool:output");
    let content = updates[0].item["detail"]["output"].as_str().unwrap();
    assert!(content.len() <= MAX_OUTPUT);
    assert!(output.ends_with(content));
    let stored = stream.tools.get("tool").unwrap();
    assert_eq!(stored[0].1["detail"]["content"], "");
    assert_eq!(stored[1].1["detail"]["output"], "");
}

#[test]
fn action_rows_share_the_existing_stream_capacity_and_release_it_on_completion() {
    let mut stream = Stream::default();
    for index in 0..MAX_ITEMS - 2 {
        progress(
            &mut stream,
            "item/started",
            &json!({"turnId":"turn","item":{
                "type":"commandExecution","id":index.to_string(),"command":"echo"
            }}),
        );
    }
    let multiple = json!({"turnId":"turn","item":{
    "type":"commandExecution","id":"multiple","command":"wrapped",
    "commandActions":[
        {"type":"unknown","command":"first"},
        {"type":"unknown","command":"second"}
    ]}});
    assert_eq!(
        stream.progress("item/started", &multiple),
        Err(AgentSessionError::Failed)
    );
    assert!(stream.events.is_empty());
    assert!(stream.complete(&json!({"id":"0"})).unwrap());
    assert_eq!(progress(&mut stream, "item/started", &multiple).len(), 3);
    assert!(stream.complete(&multiple["item"]).unwrap());
    assert!(!stream.complete(&multiple["item"]).unwrap());
    assert_eq!(stream.tool_rows, MAX_ITEMS - 3);
}

#[test]
fn streamed_batch_stdout_does_not_overwrite_an_independent_read() {
    let mut stream = Stream::default();
    let running = progress(
        &mut stream,
        "item/started",
        &json!({"turnId":"turn","item":{
            "type":"commandExecution","id":"batch","command":"cat a.rs && cat b.ts",
            "commandActions":[
                {"type":"read","command":"cat a.rs","path":"a.rs"},
                {"type":"read","command":"cat b.ts","path":"b.ts"}
            ]
        }}),
    );
    progress(
        &mut stream,
        "item/started",
        &json!({"turnId":"turn","item":{
            "type":"commandExecution","id":"single","command":"cat c.ts",
            "commandActions":[{"type":"read","command":"cat c.ts","path":"c.ts"}]
        }}),
    );

    let first = progress(
        &mut stream,
        "item/commandExecution/outputDelta",
        &params("batch", "use std::sync::Arc;\n"),
    );
    let independent = progress(
        &mut stream,
        "item/commandExecution/outputDelta",
        &params("single", "export const value = 1;\n"),
    );
    let second = progress(
        &mut stream,
        "item/commandExecution/outputDelta",
        &params("batch", "import type { WebContents } from 'electron';\n"),
    );

    assert_eq!(running.len(), 3);
    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].key, "native:turn:batch:output");
    assert_eq!(
        second[0].item["detail"]["output"],
        "use std::sync::Arc;\nimport type { WebContents } from 'electron';\n"
    );
    for (_, action) in &stream.tools.get("batch").unwrap()[..2] {
        assert_eq!(action["detail"]["content"], "");
    }
    assert_eq!(independent[0].key, "native:turn:single");
    assert_eq!(
        stream.tools.get("single").unwrap()[0].1["detail"]["content"],
        "export const value = 1;\n"
    );
}
