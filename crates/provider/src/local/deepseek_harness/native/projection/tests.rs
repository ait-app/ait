use super::*;

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
