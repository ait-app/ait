use super::*;

#[test]
fn projection_errors_describe_the_cause_without_native_payloads() {
    let mut stream = Stream::new("session".to_owned(), ImageStore::default());
    for (message, expected) in [
        (
            json!({"role":"assistant","content":[]}),
            "message.timestamp",
        ),
        (
            json!({"role":"assistant","timestamp":1000,"content":[{"type":"toolCall","id":""}]}),
            "toolCall.id",
        ),
        (
            json!({"role":"assistant","timestamp":1000,"content":[{"type":"text","text":null}]}),
            "message.content.text/thinking",
        ),
        (
            json!({"role":"assistant","timestamp":1000,"content":[],"usage":{"input":1,"output":1,"cost":{"total":-1}}}),
            "invalid token usage or cost",
        ),
    ] {
        let error = stream.message(&message).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("fixture-secret"));
    }
    assert!(matches!(
        stream.text(&json!([{"type":"image","data":"invalid"}])),
        Err(Error::Image)
    ));
    stream.tools = (0..4096)
        .map(|index| (index.to_string(), Tool::new(json!({}))))
        .collect();
    assert!(matches!(stream.message(&json!({"role":"assistant","timestamp":1000,"content":[{"type":"toolCall","id":"overflow"}]})), Err(Error::ToolLimit)));
}

#[test]
fn streaming_and_replay_use_the_same_keys_and_preserve_equal_timestamps() {
    let mut stream = Stream::new("session".to_owned(), ImageStore::default());
    let message = json!({"role":"assistant","timestamp":1000,"content":[{"type":"thinking","thinking":"Plan"},{"type":"text","text":"Hello"}],"stopReason":"stop"});
    stream
        .consume(&json!({"type":"message_start","message":message}))
        .unwrap();
    stream.consume(&json!({"type":"message_update","message":message,"assistantMessageEvent":{"type":"text_delta","contentIndex":1,"delta":"Hello"}})).unwrap();
    let AgentTurnEvent::Progress {
        entry: progress, ..
    } = stream.events.pop_front().unwrap()
    else {
        panic!("expected progress")
    };
    stream.message(&message).unwrap();
    let AgentTurnEvent::Timeline(entry) = stream.events.pop_back().unwrap() else {
        panic!("expected final")
    };
    assert_eq!(entry.key, progress.key);
    stream.message(&message).unwrap();
    let AgentTurnEvent::Timeline(second) = stream.events.pop_back().unwrap() else {
        panic!("expected final")
    };
    assert_ne!(entry.key, second.key);
}

#[test]
fn tool_failures_usage_images_and_invalid_native_messages_are_bounded() {
    let root = tempfile::tempdir().unwrap();
    let mut stream = Stream::new(
        "session".to_owned(),
        ImageStore::new(root.path().join("images")),
    );
    stream.message(&json!({"role":"assistant","timestamp":1000,"content":[{"type":"toolCall","id":"call","name":"bash","arguments":{"command":"ls"}}],"usage":{"input":10,"output":2,"cacheRead":3,"cost":{"total":0.1}}})).unwrap();
    stream.message(&json!({"role":"toolResult","timestamp":1001,"toolCallId":"call","content":[{"type":"text","text":"failed"}],"isError":true})).unwrap();
    assert!(
        matches!(stream.events.back(),Some(AgentTurnEvent::Timeline(entry)) if entry.item["status"]=="failed")
    );
    assert!(
        stream
            .message(&json!({"role":"assistant","content":[]}))
            .is_err()
    );
    // O13: a result without a known call still shows, under its native tool name.
    stream
        .message(
            &json!({"role":"toolResult","timestamp":1002,"toolCallId":"unknown",
            "toolName":"grep","content":[]}),
        )
        .unwrap();
    assert!(
        matches!(stream.events.back(),Some(AgentTurnEvent::Timeline(entry))
            if entry.item["callId"]=="unknown" && entry.item["name"]=="grep"
                && entry.item["status"]=="completed")
    );
    assert!(preview(&"x".repeat(200_000)).len() < 150_000);
    assert!(
        stream
            .text(&json!([{"type":"image","data":"aW1hZ2U=","mimeType":"image/png"}]))
            .unwrap()
            .contains("Image")
    );
}

#[test]
fn persisted_blob_images_match_inline_rendering_and_reject_corrupt_references() {
    let root = tempfile::tempdir().unwrap();
    let blobs = root.path().join("blobs");
    std::fs::create_dir(&blobs).unwrap();
    let bytes = b"fixture-image";
    let hash = format!("{:x}", Sha256::digest(bytes));
    std::fs::write(blobs.join(&hash), bytes).unwrap();
    let stream = Stream::new(
        "session".to_owned(),
        ImageStore::new(root.path().join("images")),
    )
    .with_blobs(blobs.clone());
    let inline = json!({"type":"image","mimeType":"image/png","data":base64::engine::general_purpose::STANDARD.encode(bytes)});
    let blob = json!({"type":"image","mimeType":"image/png","data":format!("blob:sha256:{hash}")});
    assert_eq!(stream.image(&blob).unwrap(), stream.image(&inline).unwrap());
    std::fs::write(blobs.join(hash), b"tampered").unwrap();
    assert!(matches!(stream.image(&blob), Err(Error::Image)));
    assert!(matches!(
        stream.image(&json!({"data":"blob:sha256:../outside"})),
        Err(Error::Image)
    ));
}

#[test]
fn arbitrary_extension_messages_share_projection_without_plugin_name_rules() {
    let mut stream = Stream::new("plugins".to_owned(), ImageStore::default());
    for (index, custom_type) in ["example.extension.alpha", "another-vendor-output"]
        .iter()
        .enumerate()
    {
        let message = json!({"role":"custom","customType":custom_type,"display":true,
            "content":[{"type":"text","text":format!("output {index}")}],"timestamp":1000+index});
        stream
            .consume(&json!({"type":"message_start","message":message}))
            .unwrap();
        stream
            .consume(&json!({"type":"message_end","message":message}))
            .unwrap();
    }
    let events = timeline(&mut stream);
    assert_eq!(events.len(), 2);
    for (index, entry) in events.iter().enumerate() {
        // D11: a plain-text row named after the custom type, keyed for replay.
        assert_eq!(entry.item["type"], "tool_call");
        assert_eq!(entry.item["callId"], entry.key);
        assert_eq!(entry.item["detail"]["text"], format!("output {index}"));
        assert_eq!(
            entry.item["metadata"]["customType"],
            ["example.extension.alpha", "another-vendor-output"][index]
        );
    }
    stream.message(&json!({"role":"custom","display":false,"content":"hidden private context","timestamp":2000})).unwrap();
    stream
        .message(&json!({"role":"custom","customType":"empty","content":[],"timestamp":3000}))
        .unwrap();
    assert!(stream.events.is_empty());
}

#[test]
fn native_command_output_and_plugin_notifications_are_visible_without_history_messages() {
    let mut stream = Stream::new("session".to_owned(), ImageStore::default());
    stream.begin("turn".to_owned(), None);
    stream.start_run();
    for frame in [
        json!({"type":"command_output","text":"Native command result"}),
        json!({"type":"extension_ui_request","method":"notify","message":"Plugin notification","notifyType":"warning"}),
    ] {
        if frame["type"] == "command_output" {
            stream.command_output(&frame).unwrap();
        } else {
            stream.consume(&frame).unwrap();
        }
        let AgentTurnEvent::Timeline(entry) = stream.events.pop_back().unwrap() else {
            panic!("expected visible native output");
        };
        assert_eq!(entry.turn_id.as_deref(), Some("turn"));
        assert_eq!(entry.item["metadata"]["transient"], true);
        assert_eq!(entry.item["metadata"]["nativeType"], frame["type"]);
        assert_eq!(
            entry.item.get("text").unwrap_or(&entry.item["message"]),
            frame.get("text").unwrap_or(&frame["message"])
        );
        if frame["type"] == "extension_ui_request" {
            assert_eq!(entry.item["type"], "notification");
            assert_eq!(entry.item["level"], "warning");
        }
    }
    assert_eq!(stream.last_text.as_deref(), Some("Native command result"));
    assert!(
        stream
            .command_output(&json!({"type":"command_output","text":null}))
            .is_err()
    );
    let count = stream.events.len();
    stream
        .command_output(&json!({"type":"command_output","text":" \x1b[0m\n"}))
        .unwrap();
    assert_eq!(stream.events.len(), count);
}

#[test]
fn background_results_are_notifications_that_do_not_end_the_turn() {
    let mut stream = Stream::new("session".to_owned(), ImageStore::default());
    stream.last_text = Some("Assistant reply".to_owned());
    // O10: OMP 18.18.8 delivers a settled background job as a `<system-notice>`.
    let message = json!({"role":"custom","customType":"async-result","display":true,
        "attribution":"agent","timestamp":1000,
        "content":"<system-notice>\nBackground job bg_4 (make) has completed. Resume.\nraw output\n</system-notice>",
        "details":{"jobs":[{"jobId":"bg_4","type":"bash","status":"completed","label":"make"}]}});
    stream.message(&message).unwrap();
    let rows = timeline(&mut stream);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].key, "native:omp:session:custom:1000:0");
    assert_eq!(
        rows[0].item,
        json!({"type":"notification","level":"info",
            "message":"Background job bg_4 (make) has completed. Resume.\nraw output"})
    );
    assert_eq!(stream.last_text.as_deref(), Some("Assistant reply"));
    assert!(!stream.failed);
    assert!(!stream.aborted);
}

#[test]
fn command_output_waits_for_the_run_and_a_local_prompt_is_echoed_with_its_client_identity() {
    let mut stream = Stream::new("session".to_owned(), ImageStore::default());
    stream.begin("turn".to_owned(), Some("/session".to_owned()));
    stream
        .pending_inputs
        .push_back(("prompt".to_owned(), Some("client-1".to_owned())));
    stream.handled_locally("prompt");
    assert!(stream.pending_inputs.is_empty());
    let frame = json!({"type":"command_output",
        "text":"\u{1b}[1mSession\u{1b}[0m: \u{1b}]8;;file:///x\u{7}local\u{1b}]8;;\u{1b}\\ \n"});

    stream.command_output(&frame).unwrap();
    assert!(stream.events.is_empty());
    stream.settle_prompt();

    let items: Vec<_> = stream
        .events
        .drain(..)
        .map(|event| match event {
            AgentTurnEvent::Timeline(entry) => entry.item,
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["type"], "user_message");
    assert_eq!(items[0]["text"], "/session");
    assert_eq!(items[0]["clientMessageId"], "client-1");
    assert_eq!(items[1]["type"], "assistant_message");
    assert_eq!(items[1]["text"], "Session: local");
    assert_eq!(stream.last_text.as_deref(), Some("Session: local"));
    // A second settlement neither echoes again nor repeats output.
    stream.settle_prompt();
    assert!(stream.events.is_empty());
}

#[test]
fn a_native_user_message_replaces_the_local_echo_and_started_runs_show_output_at_once() {
    let mut stream = Stream::new("session".to_owned(), ImageStore::default());
    stream.begin("turn".to_owned(), Some("/compact".to_owned()));
    stream.pending_inputs.push_back(("prompt".to_owned(), None));
    stream
        .command_output(&json!({"type":"command_output","text":"before"}))
        .unwrap();
    stream.start_run();
    assert!(
        matches!(stream.events.pop_front(), Some(AgentTurnEvent::Timeline(entry)) if entry.item["text"] == "before")
    );
    stream
        .command_output(&json!({"type":"command_output","text":"after"}))
        .unwrap();
    assert_eq!(stream.events.len(), 1);
    stream
        .message(&json!({"role":"user","timestamp":1000,"content":"/compact"}))
        .unwrap();
    stream.events.clear();
    stream.handled_locally("prompt");

    stream.settle_prompt();

    assert!(stream.events.is_empty());
}

#[test]
fn ansi_stripping_keeps_plain_and_multibyte_text() {
    assert!(matches!(strip_ansi("plain ü"), Cow::Borrowed("plain ü")));
    assert_eq!(strip_ansi("\u{1b}[38;5;208mü\u{1b}[0m\u{1b}(B\u{1b}="), "ü");
    assert_eq!(strip_ansi("a\u{1b}]0;title\u{7}b"), "ab");
    assert_eq!(strip_ansi("unterminated \u{1b}["), "unterminated ");
}

#[test]
fn an_interrupted_turn_cancels_tools_that_never_reported_a_result() {
    let mut stream = Stream::new("session".to_owned(), ImageStore::default());
    stream
        .message(&json!({"role":"assistant","timestamp":1000,"content":[
        {"type":"toolCall","id":"called","name":"bash","arguments":{"command":"sleep 30"}}]}))
        .unwrap();
    stream
        .consume(
            &json!({"type":"tool_execution_start","toolCallId":"started",
            "toolName":"read","args":{"path":"file.rs"}}),
        )
        .unwrap();
    stream.events.clear();

    stream.terminalize();

    let rows: Vec<_> = stream
        .events
        .drain(..)
        .map(|event| match event {
            AgentTurnEvent::Timeline(entry) => entry,
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(rows.len(), 2);
    for (row, id) in rows.iter().zip(["called", "started"]) {
        assert_eq!(row.key, format!("native:omp:session:tool:{id}"));
        assert_eq!(row.item["status"], "canceled");
        assert!(row.item["error"].is_null());
    }
    assert!(stream.tools.is_empty());
    stream.terminalize();
    assert!(stream.events.is_empty());
}

/// Drain the queued timeline items, failing on any other event.
fn timeline(stream: &mut Stream) -> Vec<NativeItem> {
    stream
        .events
        .drain(..)
        .map(|event| match event {
            AgentTurnEvent::Timeline(entry) => entry,
            other => panic!("unexpected {other:?}"),
        })
        .collect()
}

fn user(timestamp: i64, text: &str) -> Value {
    json!({"role":"user","timestamp":timestamp,"content":[{"type":"text","text":text}]})
}

#[test]
fn native_entry_ids_name_user_messages_and_repeated_announcements_show_once() {
    let mut stream = Stream::new("session".to_owned(), ImageStore::default());
    stream.pending_inputs.extend([
        ("p1".to_owned(), Some("c1".to_owned())),
        ("p2".to_owned(), None),
    ]);
    let mut first = user(1000, "hello");
    first["id"] = json!("entry-1");
    let mut second = user(2000, "again");
    second["entryId"] = json!("entry-2");

    for message in [&first, &first, &second, &second] {
        stream.message(message).unwrap();
    }

    let items = timeline(&mut stream);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].key, "native:omp:session:user:1000:0");
    assert_eq!(items[0].item["messageId"], "entry-1");
    assert_eq!(items[0].item["clientMessageId"], "c1");
    assert_eq!(items[1].key, "native:omp:session:user:2000:0");
    assert_eq!(items[1].item["messageId"], "entry-2");
    assert!(items[1].item.get("clientMessageId").is_none());
    // A repeat consumes no pending input and needs no lookup.
    assert!(stream.pending_inputs.is_empty());
    assert_eq!(stream.unresolved(), 0);
}

#[test]
fn messages_without_entry_ids_are_recognised_by_anchor_and_text() {
    let mut stream = Stream::new("session".to_owned(), ImageStore::default());
    stream.pending_inputs.extend([
        ("p1".to_owned(), Some("c1".to_owned())),
        ("p2".to_owned(), Some("c2".to_owned())),
    ]);

    for message in [user(1000, "same"), user(1000, "same"), user(1000, "other")] {
        stream.message(&message).unwrap();
    }

    let items = timeline(&mut stream);
    assert_eq!(items.len(), 2);
    // Without lookups the anchor key stays the message id, as before D9.
    assert_eq!(items[0].item["messageId"], items[0].key);
    assert_eq!(items[0].item["clientMessageId"], "c1");
    assert_eq!(items[1].key, "native:omp:session:user:1000:1");
    assert_eq!(items[1].item["clientMessageId"], "c2");
    assert_eq!(stream.unresolved(), 0);
}

#[test]
fn held_user_messages_keep_native_order_until_their_entries_are_named() {
    let mut stream =
        Stream::new("session".to_owned(), ImageStore::default()).with_identity_lookups();
    stream.message(&user(1000, "same")).unwrap();
    stream.message(&user(1010, "same")).unwrap();
    stream
        .message(&json!({"role":"assistant","timestamp":1020,
            "content":[{"type":"text","text":"reply"}],"stopReason":"stop"}))
        .unwrap();
    assert_eq!(stream.unresolved(), 2);
    assert!(stream.next_event().is_none());

    // OMP lists entries in file order; text blocks join without a separator.
    let reply = json!([{"entryId":"old","text":"same"},{"entryId":"first","text":"same"},
        {"entryId":"second","text":"same"},{"entryId":"other","text":"different"}]);
    stream.resolve_identities(2, Some(&reply));

    let mut ids = Vec::new();
    while let Some(event) = stream.next_event() {
        let AgentTurnEvent::Timeline(entry) = event else {
            panic!("unexpected {event:?}");
        };
        ids.push(entry.item["messageId"].clone());
    }
    assert_eq!(
        ids,
        [
            json!("first"),
            json!("second"),
            json!("native:omp:session:assistant:1020:0:0")
        ]
    );
    // A later repeat without an entry id is still recognised by its anchor and text.
    stream.message(&user(1010, "same")).unwrap();
    assert!(stream.events.is_empty());
}

#[test]
fn unnamed_or_ambiguous_held_messages_fall_back_to_their_anchor_keys() {
    let mut stream =
        Stream::new("session".to_owned(), ImageStore::default()).with_identity_lookups();
    let mut shown = user(1000, "repeat");
    shown["id"] = json!("entry-1");
    stream.message(&shown).unwrap();
    stream
        .message(&json!({"role":"user","timestamp":2000,
            "content":[{"type":"text","text":"re"},{"type":"text","text":"peat"}]}))
        .unwrap();
    stream.message(&user(3000, "lost")).unwrap();

    // The newest entry for "repeat" already names a shown message, so the held one is not
    // given an older guess; "lost" has no entry at all.
    let reply = json!([{"entryId":"older","text":"repeat"},{"entryId":"entry-1","text":"repeat"}]);
    stream.resolve_identities(1, Some(&reply));
    stream.resolve_identities(1, None);

    let items = timeline(&mut stream);
    assert_eq!(items[0].item["messageId"], "entry-1");
    assert_eq!(items[1].item["messageId"], items[1].key);
    assert_eq!(items[1].item["text"], "re\npeat");
    assert_eq!(items[2].item["messageId"], items[2].key);
    assert_eq!(stream.unresolved(), 0);
}

fn skill(args: Option<&str>, attribution: &str) -> Value {
    json!({"role":"custom","customType":"skill-prompt","timestamp":1000,"display":true,
        "attribution":attribution,"content":"[IMPORTANT: User invoked the \"review\" skill]",
        "details":{"name":"review","path":"/skills/review/SKILL.md","args":args,
            "prompt":"/skill:review x","lineCount":1}})
}

#[test]
fn skill_prompts_echo_the_typed_command_and_consume_their_pending_input() {
    let mut stream =
        Stream::new("session".to_owned(), ImageStore::default()).with_identity_lookups();
    stream.pending_inputs.extend([
        ("prompt".to_owned(), Some("skill-input".to_owned())),
        ("steer".to_owned(), Some("steer-input".to_owned())),
    ]);

    stream.message(&skill(Some(" x "), "user")).unwrap();
    stream.message(&skill(Some(" x "), "user")).unwrap();
    stream.message(&user(1010, "follow up")).unwrap();

    let items = timeline(&mut stream);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].key, "native:omp:session:custom:1000:0");
    assert_eq!(
        items[0].item,
        json!({"type":"user_message","messageId":items[0].key,"text":"/skill:review x",
            "clientMessageId":"skill-input"})
    );
    assert_eq!(items[1].item["clientMessageId"], "steer-input");
    assert!(stream.pending_inputs.is_empty());
}

#[test]
fn only_user_attributed_skill_prompts_with_a_name_become_user_messages() {
    let mut stream = Stream::new("session".to_owned(), ImageStore::default());
    stream.message(&skill(None, "user")).unwrap();
    let mut unnamed = skill(Some("x"), "user");
    unnamed["details"]["name"] = json!("");
    unnamed["timestamp"] = json!(2000);
    let mut agent = skill(Some("x"), "agent");
    agent["timestamp"] = json!(3000);
    stream.message(&unnamed).unwrap();
    stream.message(&agent).unwrap();

    let items = timeline(&mut stream);
    assert_eq!(items[0].item["text"], "/skill:review");
    assert_eq!(items[0].item["type"], "user_message");
    assert!(
        items[1..]
            .iter()
            .all(|entry| entry.item["type"] == "tool_call"
                && entry.item["metadata"]["customType"] == "skill-prompt")
    );
}

/// Real OMP 18.18.8 frames for one parallel round: a read and a grep whose executions end
/// out of call order, while their `toolResult` messages follow call order.
fn parallel_round() -> (Value, Vec<Value>) {
    let assistant = json!({"role":"assistant","timestamp":1000,"stopReason":"toolUse","content":[
        {"type":"toolCall","id":"r","name":"read","arguments":{"i":"r","path":"a.txt"}},
        {"type":"toolCall","id":"g","name":"grep","arguments":{"i":"g","pattern":"line"}}]});
    let read = json!({"content":[{"type":"text","text":"[a.txt#0097]\n1:line"}],
        "details":{"displayContent":{"text":"line","startLine":1}}});
    let grep = json!({"content":[{"type":"text","text":"# a.txt\n*1:line"}],
        "details":{"matchCount":1}});
    let result = |id: &str, name: &str, result: &Value, timestamp: i64| {
        json!({"role":"toolResult","toolCallId":id,"toolName":name,"content":result["content"],
            "details":result["details"],"isError":false,"timestamp":timestamp})
    };
    let frames = vec![
        json!({"type":"message_end","message":assistant}),
        json!({"type":"tool_execution_start","toolCallId":"r","toolName":"read","args":{"path":"a.txt"}}),
        json!({"type":"tool_execution_start","toolCallId":"g","toolName":"grep","args":{"pattern":"line"}}),
        json!({"type":"tool_execution_end","toolCallId":"g","toolName":"grep","result":grep,"isError":false}),
        json!({"type":"tool_execution_update","toolCallId":"g","toolName":"grep","partialResult":grep}),
        json!({"type":"tool_execution_end","toolCallId":"r","toolName":"read","result":read,"isError":false}),
        json!({"type":"message_end","message":result("r", "read", &read, 1001)}),
        json!({"type":"message_end","message":result("g", "grep", &grep, 1002)}),
    ];
    (assistant, frames)
}

#[test]
fn tool_execution_end_completes_live_rows_that_replay_records_in_result_order() {
    let mut live = Stream::new("session".to_owned(), ImageStore::default());
    let (_, frames) = parallel_round();
    for frame in &frames {
        live.consume(frame).unwrap();
    }
    let mut progress = Vec::new();
    let mut rows = Vec::new();
    for event in live.events.drain(..) {
        match event {
            AgentTurnEvent::Progress { entry, .. } => progress.push(entry),
            AgentTurnEvent::Timeline(entry) => rows.push(entry),
            other => panic!("unexpected {other:?}"),
        }
    }
    // Running, then completed at once by tool_execution_end; the late update is ignored.
    let statuses: Vec<_> = progress
        .iter()
        .map(|entry| {
            (
                entry.item["callId"].as_str().unwrap(),
                entry.item["status"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        statuses,
        [
            ("r", "running"),
            ("g", "running"),
            ("g", "completed"),
            ("r", "completed")
        ]
    );
    assert_eq!(
        progress[3].item["detail"],
        json!({"type":"read","filePath":"a.txt","content":"line"})
    );
    assert_eq!(
        rows.iter().map(|row| row.key.as_str()).collect::<Vec<_>>(),
        ["native:omp:session:tool:r", "native:omp:session:tool:g"]
    );
    assert_eq!(rows[0].item, progress[3].item);
    assert_eq!(rows[1].item, progress[2].item);
    assert!(live.tools.is_empty());

    // History replays the same messages and shows identical rows.
    let mut replay = Stream::new("session".to_owned(), ImageStore::default());
    for frame in frames.iter().filter(|frame| frame["type"] == "message_end") {
        replay.message(&frame["message"]).unwrap();
    }
    assert_eq!(timeline(&mut replay), rows);
}

#[test]
fn an_ended_tool_keeps_its_outcome_when_the_turn_stops_before_its_result_message() {
    let mut stream = Stream::new("session".to_owned(), ImageStore::default());
    let (_, frames) = parallel_round();
    for frame in &frames[..6] {
        stream.consume(frame).unwrap();
    }
    stream
        .consume(
            &json!({"type":"tool_execution_end","toolCallId":"r","toolName":"read",
            "result":{"content":[]},"isError":false}),
        )
        .unwrap();
    stream.events.clear();

    stream.terminalize();

    let rows = timeline(&mut stream);
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.item["status"] == "completed"));
}

#[test]
fn failed_tools_carry_paseo_failure_text_and_web_search_errors_fail() {
    let mut stream = Stream::new("session".to_owned(), ImageStore::default());
    stream
        .message(&json!({"role":"assistant","timestamp":1000,"content":[
        {"type":"toolCall","id":"s","name":"web_search","arguments":{"query":"rust"}},
        {"type":"toolCall","id":"y","name":"yield","arguments":{}}]}))
        .unwrap();
    stream
        .consume(
            &json!({"type":"tool_execution_end","toolCallId":"s","toolName":"web_search",
            "result":{"content":[],"details":{"error":"All providers failed"}},"isError":false}),
        )
        .unwrap();
    let Some(AgentTurnEvent::Progress { entry, .. }) = stream.events.pop_back() else {
        panic!("expected progress");
    };
    assert_eq!(entry.item["status"], "failed");
    assert_eq!(entry.item["error"], "All providers failed");
    // OMP 18.18.8 for a tool the agent lacks.
    stream
        .message(
            &json!({"role":"toolResult","toolCallId":"y","toolName":"yield","timestamp":1001,
            "content":[{"type":"text","text":"Tool yield not found"}],
            "details":{"isError":true,"error":"Tool yield not found"},"isError":true}),
        )
        .unwrap();
    let rows = timeline(&mut stream);
    assert_eq!(rows[0].item["status"], "failed");
    assert_eq!(rows[0].item["error"], "Tool yield not found");
}

fn todo_result(id: &str, timestamp: i64, phases: Value) -> Value {
    let mut result = json!({"role":"toolResult","toolCallId":id,"toolName":"todo",
        "timestamp":timestamp,"content":[{"type":"text","text":"Remaining items"}],
        "details":{"op":"init","storage":"session"},"isError":false});
    result["details"]["phases"] = phases;
    result
}

#[test]
fn todo_results_and_reminders_show_lists_without_tool_rows_or_repeats() {
    let mut stream = Stream::new("session".to_owned(), ImageStore::default());
    stream.begin("turn".to_owned(), None);
    let phases = json!([{"name":"Build","tasks":[{"content":"one","status":"in_progress"},
        {"content":"two","status":"pending"}]}]);
    stream
        .message(&json!({"role":"assistant","timestamp":1000,"content":[
        {"type":"toolCall","id":"t1","name":"todo","arguments":{"op":"init"}}]}))
        .unwrap();
    stream
        .consume(
            &json!({"type":"tool_execution_start","toolCallId":"t1","toolName":"todo",
            "args":{"op":"init"}}),
        )
        .unwrap();
    stream
        .consume(
            &json!({"type":"tool_execution_end","toolCallId":"t1","toolName":"todo",
            "result":{"content":[],"details":{"phases":phases}},"isError":false}),
        )
        .unwrap();
    // The todo tool has no row of its own.
    assert!(stream.events.is_empty());
    stream
        .message(&todo_result("t1", 1001, phases.clone()))
        .unwrap();
    stream
        .message(&todo_result("t2", 1002, phases.clone()))
        .unwrap();
    let reminder = json!({"type":"todo_reminder","todos":[{"content":"two","status":"pending"}],
        "attempt":1,"maxAttempts":3});
    stream.consume(&reminder).unwrap();
    stream.consume(&reminder).unwrap();
    stream.message(&todo_result("t3", 1003, phases)).unwrap();

    let rows = timeline(&mut stream);
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(rows[0].key, "native:omp:session:todo:t1");
    assert_eq!(rows[0].item["items"][0]["text"], "one");
    assert!(rows[0].turn_id.is_none());
    assert_eq!(rows[1].item["items"].as_array().unwrap().len(), 1);
    assert_eq!(rows[1].turn_id.as_deref(), Some("turn"));
    assert!(stream.tools.is_empty());
}

#[test]
fn telemetry_events_show_rows_and_compactions_pair_their_lifecycle() {
    let mut stream = Stream::new("session".to_owned(), ImageStore::default());
    stream.begin("turn".to_owned(), None);
    for frame in [
        json!({"type":"notice","level":"warning","message":"Cost warning","source":"cost-gate"}),
        json!({"type":"auto_compaction_start","reason":"overflow","action":"shake"}),
        json!({"type":"auto_compaction_end","action":"shake","aborted":false,"skipped":true}),
        json!({"type":"auto_compaction_start","reason":"threshold","action":"handoff"}),
        json!({"type":"auto_compaction_end","result":{"tokensBefore":100_010}}),
        json!({"type":"auto_retry_start","attempt":1,"maxAttempts":3,"delayMs":200,
            "errorMessage":"overloaded"}),
        json!({"type":"auto_retry_end","attempt":1,"success":true}),
        json!({"type":"notice","level":"verbose","message":"dropped"}),
        json!({"type":"future_event"}),
    ] {
        stream.consume(&frame).unwrap();
    }
    let rows = timeline(&mut stream);
    let kinds: Vec<_> = rows
        .iter()
        .map(|row| row.item["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        [
            "notification",
            "compaction",
            "compaction",
            "compaction",
            "compaction",
            "tool_call",
            "tool_call"
        ]
    );
    assert!(
        rows.iter()
            .all(|row| row.turn_id.as_deref() == Some("turn"))
    );
    assert_eq!(rows[1].item["compactionId"], rows[2].item["compactionId"]);
    assert_eq!(rows[3].item["compactionId"], rows[4].item["compactionId"]);
    assert_ne!(rows[1].item["compactionId"], rows[3].item["compactionId"]);
    assert_eq!(rows[4].item["preTokens"], 100_010);
    assert_eq!(rows[5].item["callId"], rows[6].item["callId"]);
    assert_ne!(rows[5].key, rows[6].key);

    // A compaction an Ait-issued /compact owns shows only its own rows.
    stream.compactions.manual = true;
    stream
        .consume(&json!({"type":"manual_compaction_start"}))
        .unwrap();
    stream
        .consume(&json!({"type":"auto_compaction_start","reason":"overflow"}))
        .unwrap();
    assert_eq!(timeline(&mut stream).len(), 1);
}

#[test]
fn omp_state_todos_show_once_unless_a_list_already_shows_them() {
    let mut stream = Stream::new("session".to_owned(), ImageStore::default());
    let phases = json!([{"name":"Plan","tasks":[{"content":"one","status":"pending"}]}]);
    stream
        .message(&todo_result("t1", 1000, phases.clone()))
        .unwrap();
    timeline(&mut stream);

    stream.state_todos(&json!({"todoPhases":phases}));
    stream.state_todos(&json!({"todoPhases":[]}));
    stream.state_todos(&json!({}));
    assert!(stream.events.is_empty());

    let changed = json!([{"name":"Plan","tasks":[{"content":"one","status":"completed"}]}]);
    stream.state_todos(&json!({"todoPhases":changed}));
    stream.state_todos(&json!({"todoPhases":changed}));
    let rows = timeline(&mut stream);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].key.contains(":notice:"));
    assert_eq!(rows[0].item["items"][0]["completed"], true);
}

fn task_call() -> Value {
    json!({"role":"assistant","timestamp":1000,"stopReason":"toolUse","content":[
        {"type":"toolCall","id":"task-1","name":"task","arguments":{"context":"# Goal",
            "tasks":[{"name":"worker"}]}}]})
}

#[test]
fn task_cards_name_their_first_child_by_its_qualified_identity() {
    let mut stream = Stream::new("root".to_owned(), ImageStore::default());
    stream.message(&task_call()).unwrap();
    stream
        .consume(
            &json!({"type":"tool_execution_start","toolCallId":"task-1","toolName":"task",
            "args":task_call()["content"][0]["arguments"]}),
        )
        .unwrap();
    stream.events.clear();

    // Subagent events name the running card's child, and describe a card without its own
    // description; a later index does not replace the child.
    stream.task_child("task-1", 1, "second", Some("Second"));
    stream.task_child("task-1", 0, "worker", Some("Review the change"));
    stream.task_child("task-1", 1, "second", None);
    stream.task_child("task-1", 0, "worker", Some("Review the change"));
    stream.task_child("unknown", 0, "worker", None);
    let running: Vec<_> = stream
        .events
        .drain(..)
        .map(|event| match event {
            AgentTurnEvent::Progress { entry, .. } => entry.item,
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(running.len(), 2);
    assert_eq!(running[1]["status"], "running");
    assert_eq!(running[1]["detail"]["childSessionId"], "root/worker");
    assert_eq!(running[1]["detail"]["description"], "Review the change");

    // The result names the child from its details, as history does.
    let result = json!({"role":"toolResult","toolCallId":"task-1","toolName":"task",
        "timestamp":1100,"content":[{"type":"text","text":"done"}],
        "details":{"results":[{"id":"worker","exitCode":0}]}});
    stream.message(&result).unwrap();
    let rows = timeline(&mut stream);
    assert_eq!(rows[0].item["status"], "completed");
    assert_eq!(rows[0].item["detail"]["childSessionId"], "root/worker");
    assert!(rows[0].item["detail"]["description"].is_null());
    assert!(stream.cards.is_empty());
}

#[test]
fn a_local_prompt_keeps_its_client_identity_from_a_later_steer() {
    let mut stream =
        Stream::new("session".to_owned(), ImageStore::default()).with_identity_lookups();
    stream.begin("turn".to_owned(), Some("/session".to_owned()));
    stream.pending_inputs.extend([
        ("prompt".to_owned(), Some("local-input".to_owned())),
        ("steer".to_owned(), Some("steer-input".to_owned())),
    ]);

    stream.handled_locally("prompt");
    stream.message(&user(2000, "steered")).unwrap();
    stream.settle_prompt();

    let items = timeline(&mut stream);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].item["text"], "steered");
    assert_eq!(items[0].item["clientMessageId"], "steer-input");
    assert_eq!(items[1].item["text"], "/session");
    assert_eq!(items[1].item["clientMessageId"], "local-input");
}

#[test]
fn client_identities_keep_only_the_newest_messages() {
    let mut stream = Stream::new("session".to_owned(), ImageStore::default());
    stream.input_ids.extend((0..MAX_INPUT_IDS).map(|index| {
        (
            format!("native:omp:session:user:{}:0", 1_000_000 + index),
            json!(format!("client-{index}")),
        )
    }));
    // A key without a message suffix is the oldest.
    stream
        .input_ids
        .insert("legacy".to_owned(), json!("legacy-client"));
    stream
        .pending_inputs
        .push_back(("prompt".to_owned(), Some("newest".to_owned())));

    stream.message(&user(9_000_000, "newest")).unwrap();

    assert_eq!(stream.input_ids.len(), MAX_INPUT_IDS);
    assert!(!stream.input_ids.contains_key("legacy"));
    assert!(
        !stream
            .input_ids
            .contains_key("native:omp:session:user:1000000:0")
    );
    assert!(
        stream
            .input_ids
            .contains_key("native:omp:session:user:1000001:0")
    );
    assert_eq!(
        stream.input_ids["native:omp:session:user:9000000:0"],
        json!("newest")
    );
}

#[test]
fn a_completed_turn_closes_tools_whose_results_never_arrived() {
    let mut stream = Stream::new("session".to_owned(), ImageStore::default());
    stream.begin("turn".to_owned(), None);
    stream
        .consume(
            &json!({"type":"tool_execution_start","toolCallId":"ended","toolName":"bash",
            "args":{"command":"true"}}),
        )
        .unwrap();
    stream
        .consume(
            &json!({"type":"tool_execution_end","toolCallId":"ended","toolName":"bash",
            "result":{"content":[{"type":"text","text":"done"}]}}),
        )
        .unwrap();
    stream
        .consume(
            &json!({"type":"tool_execution_start","toolCallId":"running",
            "toolName":"bash","args":{"command":"sleep 1"}}),
        )
        .unwrap();
    let shown: Vec<_> = stream
        .events
        .drain(..)
        .filter_map(|event| match event {
            AgentTurnEvent::Progress { entry, .. } if entry.item["callId"] == "ended" => {
                Some(entry.item)
            }
            _ => None,
        })
        .collect();

    stream.terminalize();

    assert!(stream.tools.is_empty());
    let closed = timeline(&mut stream);
    assert_eq!(closed.len(), 2);
    let ended = closed
        .iter()
        .find(|entry| entry.key == "native:omp:session:tool:ended")
        .unwrap();
    assert_eq!(Some(&ended.item), shown.last());
    let running = closed
        .iter()
        .find(|entry| entry.key == "native:omp:session:tool:running")
        .unwrap();
    assert_eq!(running.item["status"], "canceled");
}

fn assistant(timestamp: i64, usage: &Value) -> Value {
    json!({"role":"assistant","timestamp":timestamp,"model":"m",
        "content":[{"type":"text","text":"hi"}],"usage":usage})
}

fn queued(stream: &Stream) -> Vec<provider_core::ports::usage_ledger::RequestUsage> {
    stream
        .events
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::RequestUsage(request) => Some(request.clone()),
            _ => None,
        })
        .collect()
}

fn live_stream() -> (tempfile::TempDir, Stream) {
    let root = tempfile::tempdir().unwrap();
    let mut stream = Stream::new(
        "session".to_owned(),
        ImageStore::new(root.path().join("images")),
    );
    stream.begin("turn".to_owned(), None);
    (root, stream)
}

#[test]
fn live_assistant_messages_queue_request_usage_segments() {
    let (_root, mut stream) = live_stream();
    let usage = json!({"input":10,"output":5,"cacheRead":2,"cacheWrite":1,"cost":{"total":0.1}});
    stream.message(&assistant(1000, &usage)).unwrap();
    stream.message(&assistant(1000, &usage)).unwrap();
    stream
        .message(
            &json!({"role":"assistant","timestamp":2000,"responseId":"resp",
            "content":[],"usage":{"input":1,"output":1}}),
        )
        .unwrap();

    let requests = queued(&stream);
    let ids: Vec<_> = requests.iter().map(|r| r.segment_id.as_str()).collect();
    assert_eq!(ids, ["omp:turn:1000:0", "omp:turn:1000:1", "omp:turn:resp"]);
    assert_eq!(
        (
            requests[0].input_tokens,
            requests[0].cache_read_tokens,
            requests[0].cache_write_tokens
        ),
        (10, 2, 1)
    );
    assert_eq!(requests[0].model.as_deref(), Some("m"));
    assert_eq!(requests[0].cost_usd_reported, Some(0.1));
}

#[test]
fn history_replay_and_aborted_zero_usage_queue_no_requests() {
    let root = tempfile::tempdir().unwrap();
    let mut replay = Stream::new(
        "session".to_owned(),
        ImageStore::new(root.path().join("images")),
    );
    replay
        .message(&assistant(1000, &json!({"input":10,"output":5})))
        .unwrap();
    assert!(queued(&replay).is_empty());

    let (_root, mut stream) = live_stream();
    stream
        .message(&assistant(
            1000,
            &json!({"input":0,"output":0,"cacheRead":0}),
        ))
        .unwrap();
    assert!(queued(&stream).is_empty());
}
