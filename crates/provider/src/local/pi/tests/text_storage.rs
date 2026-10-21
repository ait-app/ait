use super::*;
use crate::storage::timeline::{MAX_ENTRY_BYTES, Timeline};

#[test]
fn escaped_text_leaves_storage_headroom_for_progress_and_completion() {
    for kind in ["text", "thinking"] {
        let timeline = Timeline::memory().unwrap();
        let mut stream = streaming::Stream::default();
        stream.begin(uuid::Uuid::new_v4().to_string());
        stream.admission = true;
        let text = "\u{0001}".repeat(96 * 1024);
        for part in text.as_bytes().chunks(1024) {
            stream
                .consume(&json!({"type":"message_update","assistantMessageEvent":{
                    "type":format!("{kind}_delta"),"contentIndex":0,
                    "delta":std::str::from_utf8(part).unwrap()
                }}))
                .unwrap();
        }
        assert_eq!(stream.events.len(), 1);
        let AgentTurnEvent::Progress { observation, entry } = stream.pop().unwrap() else {
            panic!("expected coalesced progress");
        };
        assert!(serde_json::to_vec(&entry).unwrap().len() < MAX_ENTRY_BYTES);
        timeline
            .progress("agent", "pi", &observation, &entry)
            .unwrap();
        stream.end_admission();
        stream
            .consume(&json!({"type":"message_end","message":{"role":"assistant",
                "content":[{"type":kind,kind:text}]
            }}))
            .unwrap();
        let AgentTurnEvent::Timeline(entry) = stream.pop().unwrap() else {
            panic!("expected completed text");
        };
        assert!(serde_json::to_vec(&entry).unwrap().len() < MAX_ENTRY_BYTES);
        timeline.append("agent", "pi", &[entry]).unwrap();
        let (_, rows) = timeline.read("agent").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].entry.item["text"], text);
        assert_eq!(rows[1].entry.item["text"], "");
    }
}

#[test]
fn published_deltas_require_an_ordered_prefix_at_completion() {
    for complete in ["first second tail", "second first tail"] {
        let timeline = Timeline::memory().unwrap();
        let mut stream = streaming::Stream::default();
        stream.begin("turn".into());
        stream.admission = true;
        for delta in ["first ", "second "] {
            stream
                .consume(&json!({"type":"message_update","assistantMessageEvent":{
                    "type":"text_delta","contentIndex":0,"delta":delta
                }}))
                .unwrap();
            let AgentTurnEvent::Progress { observation, entry } = stream.pop().unwrap() else {
                panic!("expected progress");
            };
            timeline
                .progress("agent", "pi", &observation, &entry)
                .unwrap();
            stream.end_admission();
        }
        stream
            .consume(&json!({"type":"message_end","message":{"role":"assistant",
                "content":[{"type":"text","text":complete}]
            }}))
            .unwrap();
        let AgentTurnEvent::Timeline(entry) = stream.pop().unwrap() else {
            panic!("expected completion");
        };
        let result = timeline.append("agent", "pi", &[entry]);
        if complete.starts_with("first ") {
            result.unwrap();
            let (_, rows) = timeline.read("agent").unwrap();
            assert_eq!(rows[2].entry.item["text"], "tail");
        } else {
            assert_eq!(result, Err(model::ErrorCode::IdempotencyConflict));
        }
    }
}

#[test]
fn admission_chunk_rollover_preserves_order_and_observation_identity() {
    let mut stream = streaming::Stream::default();
    stream.begin("turn".into());
    stream.admission = true;
    let first = "a".repeat(96 * 1024);
    for delta in [first.as_str(), "second", " third"] {
        stream
            .consume(&json!({"type":"message_update","assistantMessageEvent":{
                "type":"text_delta","contentIndex":0,"delta":delta
            }}))
            .unwrap();
    }
    assert_eq!(stream.events.len(), 2);
    let timeline = Timeline::memory().unwrap();
    while let Some(AgentTurnEvent::Progress { observation, entry }) = stream.pop() {
        timeline
            .progress("agent", "pi", &observation, &entry)
            .unwrap();
    }
    let (_, rows) = timeline.read("agent").unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].entry.key, rows[1].entry.key);
    assert_eq!(rows[0].entry.item["text"], first);
    assert_eq!(rows[1].entry.item["text"], "second third");
}

#[test]
fn submitted_rich_input_keeps_identity_and_fits_storage_entries() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("images");
    let images = crate::local::images::ImageStore::new(directory.clone());
    let prompt = AgentPrompt {
        client_message_id: Some("client".into()),
        images: vec![crate::protocol::prompt::PromptImage {
            data: "aGVsbG8=".into(),
            mime_type: "image/png".into(),
        }],
        ..AgentPrompt::default()
    };
    let text = "\u{0001}".repeat(192 * 1024);
    let events = streaming::Stream::submitted(&images, "turn", &prompt, &text).unwrap();
    let timeline = Timeline::memory().unwrap();
    let mut restored = String::new();
    let mut message_ids = std::collections::BTreeSet::new();
    for event in events {
        let AgentTurnEvent::Timeline(entry) = event else {
            panic!("expected submitted timeline entry");
        };
        if restored.is_empty() {
            assert_eq!(entry.item["clientMessageId"], "client");
            assert_eq!(entry.item["messageId"], "native:pi:turn:user");
            let sources = entry.item["imageSources"].as_array().unwrap();
            assert_eq!(sources.len(), 1);
            assert_eq!(
                std::fs::read(sources[0].as_str().unwrap()).unwrap(),
                b"hello"
            );
        } else {
            assert!(entry.item.get("clientMessageId").is_none());
            assert!(entry.item.get("imageSources").is_none());
        }
        assert!(message_ids.insert(entry.item["messageId"].as_str().unwrap().to_owned()));
        assert_eq!(entry.turn_id.as_deref(), Some("turn"));
        assert!(serde_json::to_vec(&entry).unwrap().len() < MAX_ENTRY_BYTES);
        restored.push_str(entry.item["text"].as_str().unwrap());
        timeline.append("agent", "pi", &[entry]).unwrap();
    }
    assert_eq!(restored, text);
    assert_eq!(timeline.read("agent").unwrap().1.len(), 2);
    let files: Vec<_> = std::fs::read_dir(directory).unwrap().collect();
    assert_eq!(files.len(), 1);
    assert_eq!(
        std::fs::read(files[0].as_ref().unwrap().path()).unwrap(),
        b"hello"
    );
}

#[test]
fn submitted_chunks_without_client_identity_preserve_multibyte_text() {
    let text = format!("{}é尾", "a".repeat(96 * 1024 - 1));
    let events = streaming::Stream::submitted(
        &crate::local::images::ImageStore::default(),
        "turn",
        &AgentPrompt::default(),
        &text,
    )
    .unwrap();
    let mut restored = String::new();
    let mut message_ids = std::collections::BTreeSet::new();
    for event in events {
        let AgentTurnEvent::Timeline(entry) = event else {
            panic!("expected submitted timeline entry");
        };
        assert!(entry.item.get("clientMessageId").is_none());
        assert!(message_ids.insert(entry.item["messageId"].as_str().unwrap().to_owned()));
        restored.push_str(entry.item["text"].as_str().unwrap());
    }
    assert_eq!(message_ids.len(), 2);
    assert_eq!(restored, text);
}

#[tokio::test]
async fn submitted_image_only_session_is_materialized_for_timeline_display() {
    let (directory, client, spec) = fixture("");
    let image_directory = directory.path().join("images");
    let client = client.with_image_directory(image_directory.clone());
    let mut session = client.create_session(&spec).await.unwrap();
    let prompt = AgentPrompt {
        images: vec![crate::protocol::prompt::PromptImage {
            data: "aGVsbG8=".into(),
            mime_type: "image/png".into(),
        }],
        ..AgentPrompt::default()
    };

    session.start_input(&prompt, &spec.config).await.unwrap();
    let events = terminal(session.as_mut()).await;

    assert!(events.iter().any(|event| matches!(event,
        AgentTurnEvent::Timeline(entry)
            if entry.item["type"] == "user_message"
                && entry.item["text"] == ""
                && entry.item["imageSources"].as_array().is_some_and(|sources|
                    sources.len() == 1
                        && std::fs::read(sources[0].as_str().unwrap()).unwrap() == b"hello")
    )));
    let files: Vec<_> = std::fs::read_dir(image_directory).unwrap().collect();
    assert_eq!(files.len(), 1);
    assert_eq!(
        std::fs::read(files[0].as_ref().unwrap().path()).unwrap(),
        b"hello"
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn rejected_input_is_not_published_and_handled_input_is_retained() {
    let (_directory, client, spec) = fixture("");
    let mut session = client.create_session(&spec).await.unwrap();
    assert!(matches!(
        session.start_turn("reject", &spec.config).await,
        Err(AgentSessionError::Rejected)
    ));
    assert!(session.poll_turn().unwrap().is_none());
    session.start_turn("handled", &spec.config).await.unwrap();
    let events = terminal(session.as_mut()).await;
    assert!(matches!(&events[0], AgentTurnEvent::Timeline(entry)
        if entry.item["type"] == "user_message" && entry.item["text"] == "handled"));
    assert!(matches!(
        events.last(),
        Some(AgentTurnEvent::Completed(None))
    ));
    session.close().await.unwrap();
}
