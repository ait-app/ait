use super::*;

#[test]
fn unfinished_native_compaction_fails_at_turn_end_and_malformed_owner_is_rejected() {
    let mut stream = Stream::default();
    let mut tools = BTreeMap::new();
    for (seq, kind) in [(1, "compaction/start"), (2, "turn/end")] {
        apply(
            &mut stream,
            &mut tools,
            &json!({
                "seq":seq, "time":1_700_000_000_000_i64, "type":kind,
                "data":{"turn":1,"compactionId":"open"}
            }),
            "session",
        )
        .unwrap();
    }
    let statuses: Vec<_> = stream
        .events
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Timeline(entry) => entry.item["status"].as_str(),
            _ => None,
        })
        .collect();
    assert_eq!(statuses, ["loading", "failed"]);
    assert!(
        apply(
            &mut stream,
            &mut tools,
            &json!({
                "seq":3, "time":1_700_000_000_000_i64, "type":"compaction/start",
                "data":{"turn":"invalid","compactionId":"malformed"}
            }),
            "session"
        )
        .is_err()
    );
}

#[test]
fn older_compaction_journals_pair_by_opening_sequence_without_reusing_a_turn_identity() {
    let mut stream = Stream::default();
    let mut tools = BTreeMap::new();
    for (seq, kind) in [
        (1, "compaction/start"),
        (2, "compaction/end"),
        (3, "compaction/start"),
        (4, "compaction/end"),
    ] {
        apply(
            &mut stream,
            &mut tools,
            &json!({"seq":seq,"time":1_700_000_000_000_i64,
                "type":kind,"data":{"turn":1}}),
            "session",
        )
        .unwrap();
    }
    let ids: Vec<_> = stream
        .events
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Timeline(entry) => entry.item["compactionId"].as_str(),
            _ => None,
        })
        .collect();
    assert_eq!(
        ids,
        [
            "dsh:session:1",
            "dsh:session:1",
            "dsh:session:3",
            "dsh:session:3"
        ]
    );
}

#[test]
fn native_compaction_brackets_preserve_identity_time_trigger_and_failure() {
    for (turn, terminal) in [(json!(1), "completed"), (Value::Null, "failed")] {
        let mut stream = Stream::default();
        let mut tools = BTreeMap::new();
        let mut data = json!({"turn":turn,"compactionId":"compact"});
        for (seq, kind) in [(1, "compaction/start"), (2, "compaction/end")] {
            if seq == 2 && terminal == "failed" {
                data["error"] = json!("private native error");
            }
            apply(
                &mut stream,
                &mut tools,
                &json!({"seq":seq,"time":1_700_000_000_000_i64,
                    "type":kind,"data":data}),
                "session",
            )
            .unwrap();
        }
        let entries: Vec<_> = stream
            .events
            .into_iter()
            .map(|event| {
                let AgentTurnEvent::Timeline(entry) = event else {
                    panic!("compaction marker")
                };
                entry
            })
            .collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, "native:dsh:v2:session:event:1:0");
        assert_eq!(entries[1].key, "native:dsh:v2:session:event:2:0");
        assert_eq!(entries[0].item["status"], "loading");
        assert_eq!(entries[1].item["status"], terminal);
        assert_eq!(
            entries[1].item["trigger"],
            if turn.is_null() { "manual" } else { "auto" }
        );
        assert!(!entries[1].item.to_string().contains("private native error"));
    }
}

fn tool_stream() -> (Stream, BTreeMap<String, Value>) {
    let mut stream = Stream::default();
    let mut tools = BTreeMap::new();
    apply(
        &mut stream,
        &mut tools,
        &json!({"seq":1,"time":1_700_000_000_000_i64,
        "type":"tool/call","data":{"callId":"call-1","name":"write","arguments":"{}","turn":1}}),
        "session",
    )
    .unwrap();
    stream.events.clear();
    (stream, tools)
}

#[test]
fn native_and_legacy_tool_results_keep_identity_output_and_error_status() {
    for failed in [false, true] {
        for legacy in [false, true] {
            let (mut stream, mut tools) = tool_stream();
            let content = json!([{"type":"text","text":"native result"}]);
            let result = json!({"toolCallId":"call-1","isError":failed,"content":content});
            let message = if legacy {
                json!({"content":[result]})
            } else {
                result
            };
            apply(
                &mut stream,
                &mut tools,
                &json!({"seq":2,"time":1_700_000_000_001_i64,
                "type":"tool/result","data":{"turn":1,"message":message}}),
                "session",
            )
            .unwrap();
            let entry = stream
                .events
                .iter()
                .find_map(|event| match event {
                    AgentTurnEvent::Timeline(entry) if entry.item["type"] == "tool_call" => {
                        Some(entry)
                    }
                    _ => None,
                })
                .unwrap();
            assert_eq!(entry.key, "native:dsh:v2:session:tool:1:call-1");
            assert_eq!(
                entry.item["status"],
                if failed { "failed" } else { "completed" }
            );
            assert_eq!(entry.item["detail"]["output"], content);
        }
    }
}

#[test]
fn malformed_tool_results_cannot_complete_an_unrelated_call() {
    for message in [
        json!({"toolCallId":"unknown","content":[]}),
        json!({"toolCallId":null,"content":[]}),
        json!({"toolCallId":"call-1","content":null}),
        json!({"content":[{"type":"text","text":"missing identity"}]}),
    ] {
        let (mut stream, mut tools) = tool_stream();
        assert!(
            apply(
                &mut stream,
                &mut tools,
                &json!({"seq":2,"time":1_700_000_000_001_i64,
            "type":"tool/result","data":{"message":message}}),
                "session"
            )
            .is_err()
        );
    }
}
