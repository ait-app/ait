use super::*;

#[test]
fn statistics_usage_preserves_unknown_cumulative_fields_and_known_zero() {
    let mut stream = Stream::new("session".to_owned(), ImageStore::default());
    stream.add_usage(&json!({"input":1,"output":2})).unwrap();
    assert_eq!(stream.usage.cached_input_tokens, None);
    assert_eq!(stream.usage.total_cost_usd, None);
    stream
        .add_usage(&json!({"input":3,"output":4,"cacheRead":0,"cost":{"total":0}}))
        .unwrap();
    assert_eq!(stream.usage.input_tokens, Some(4));
    assert_eq!(stream.usage.output_tokens, Some(6));
    assert_eq!(stream.usage.cached_input_tokens, None);
    assert_eq!(stream.usage.total_cost_usd, None);
    assert!(
        matches!(stream.events.back(), Some(AgentTurnEvent::Usage(usage))
        if usage.cached_input_tokens.is_none() && usage.total_cost_usd.is_none())
    );

    let mut known = Stream::new("known".to_owned(), ImageStore::default());
    known
        .add_usage(&json!({"input":1,"output":2,"cacheRead":0,"cost":{"total":0}}))
        .unwrap();
    assert_eq!(known.usage.cached_input_tokens, Some(0));
    assert_eq!(known.usage.total_cost_usd, Some(0.0));
    known
        .add_usage(&json!({"input":1,"output":2,"cacheRead":3,"cost":{"total":0.1}}))
        .unwrap();
    assert_eq!(known.usage.cached_input_tokens, Some(3));
    assert_eq!(known.usage.total_cost_usd, Some(0.1));
    known.add_usage(&json!({"input":1,"output":2})).unwrap();
    assert_eq!(known.usage.cached_input_tokens, None);
    assert_eq!(known.usage.total_cost_usd, None);
}

#[test]
fn process_control_and_file_writes_keep_their_native_semantics() {
    let arguments = json!({"path":"proc://collector/kill","content":null});
    let tool = json!({"id":"process","name":"write","arguments":arguments});
    let running = tool_item(&tool, None, false, true);
    assert_eq!(running["status"], "running");
    assert_eq!(
        running["detail"],
        json!({"type":"unknown","input":arguments,"output":null})
    );
    let completed = tool_item(&tool, Some("Stopped".to_owned()), false, false);
    assert_eq!(completed["status"], "completed");
    assert_eq!(completed["detail"]["input"], arguments);
    assert_eq!(completed["detail"]["output"], "Stopped");
    let failed = tool_item(&tool, Some("Not running".to_owned()), true, false);
    assert_eq!(failed["status"], "failed");
    assert!(!failed["error"].is_null());
    let file = json!({"id":"file","name":"write","arguments":{"path":"file.rs","content":"text"}});
    assert_eq!(
        tool_item(&file, None, false, true)["detail"],
        json!({"type":"write","filePath":"file.rs","content":"text"})
    );
    let partial =
        json!({"id":"partial","name":"write","arguments":{"path":"file.rs","content":null}});
    assert_eq!(
        tool_item(&partial, None, false, true)["detail"],
        json!({"type":"write","filePath":"file.rs"})
    );
}

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
            json!({"role":"toolResult","timestamp":1000,"toolCallId":"unknown","content":"api_key=fixture-secret"}),
            "without a matching tool call",
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
        .map(|index| (index.to_string(), json!({})))
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
    assert!(
        stream
            .message(
                &json!({"role":"toolResult","timestamp":1002,"toolCallId":"unknown","content":[]})
            )
            .is_err()
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
    let events = stream.events.drain(..).collect::<Vec<_>>();
    assert_eq!(events.len(), 2);
    for (index, event) in events.iter().enumerate() {
        let AgentTurnEvent::Timeline(entry) = event else {
            panic!("expected custom timeline")
        };
        assert_eq!(entry.item["text"], format!("output {index}"));
        assert_eq!(entry.item["metadata"]["nativeRole"], "custom");
    }
    stream.message(&json!({"role":"custom","display":false,"content":"hidden private context","timestamp":2000})).unwrap();
    assert!(stream.events.is_empty());
}

#[test]
fn native_command_output_and_plugin_notifications_are_visible_without_history_messages() {
    let mut stream = Stream::new("session".to_owned(), ImageStore::default());
    stream.begin("turn".to_owned());
    for frame in [
        json!({"type":"command_output","text":"Native command result"}),
        json!({"type":"extension_ui_request","method":"notify","message":"Plugin notification","notifyType":"warning"}),
    ] {
        stream.consume(&frame).unwrap();
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
            .consume(&json!({"type":"command_output","text":null}))
            .is_err()
    );
    let count = stream.events.len();
    stream
        .consume(&json!({"type":"command_output","text":""}))
        .unwrap();
    assert_eq!(stream.events.len(), count);
}

#[test]
fn background_results_are_expandable_tools_with_native_job_identity_and_status() {
    for (status, expected) in [
        ("running", "running"),
        ("completed", "completed"),
        ("failed", "failed"),
        ("cancelled", "canceled"),
    ] {
        let mut stream = Stream::new("session".to_owned(), ImageStore::default());
        stream.last_text = Some("Assistant reply".to_owned());
        let message = json!({"role":"custom","customType":"async-result","display":true,
            "timestamp":1000,"content":"Background job bg_4 has completed. Resume your work.\nraw output",
            "details":{"jobs":[{"jobId":"bg_4","type":"bash","status":status,"label":"build"}]}});
        stream.message(&message).unwrap();
        let AgentTurnEvent::Timeline(entry) = stream.events.pop_front().unwrap() else {
            panic!("expected background tool result");
        };
        assert_eq!(entry.item["type"], "tool_call");
        assert_eq!(entry.item["callId"], "background:bg_4");
        assert_eq!(entry.item["status"], expected);
        assert_eq!(entry.item["detail"]["type"], "unknown");
        assert_eq!(entry.item["detail"]["input"], "build");
        assert_eq!(entry.item["metadata"]["jobId"], "bg_4");
        assert_eq!(entry.item["detail"]["output"], message["content"]);
        assert!(entry.item.get("text").is_none());
        assert_eq!(stream.last_text.as_deref(), Some("Assistant reply"));
        assert!(!stream.failed);
        assert!(!stream.aborted);
    }
}

#[test]
fn background_batches_and_missing_job_metadata_keep_one_bounded_result_card() {
    let mut stream = Stream::new("session".to_owned(), ImageStore::default());
    for jobs in [json!(null), json!([]), json!([{"jobId":"a"},{"jobId":"b"}])] {
        let message = json!({"role":"custom","customType":"async-result","display":true,
            "timestamp":1000,"content":"x".repeat(200_000),"details":{"jobs":jobs}});
        stream.message(&message).unwrap();
        let AgentTurnEvent::Timeline(entry) = stream.events.pop_front().unwrap() else {
            panic!("expected background result");
        };
        assert_eq!(entry.item["type"], "tool_call");
        assert!(
            entry.item["callId"]
                .as_str()
                .unwrap()
                .ends_with(":background")
        );
        assert!(entry.item["detail"]["output"].as_str().unwrap().len() < 150_000);
    }
    assert!(stream.events.is_empty());
}
