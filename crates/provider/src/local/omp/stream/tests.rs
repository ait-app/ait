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
