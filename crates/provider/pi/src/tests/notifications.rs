use super::*;

#[tokio::test]
async fn handled_extension_error_is_visible_without_failing_the_turn_or_replying() {
    let (directory, client, spec) = fixture("");
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("ui-notification-error", &spec.config)
        .await
        .unwrap();
    let events = terminal(session.as_mut()).await;
    let notices: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Timeline(entry) if entry.item["type"] == "notification" => Some(entry),
            _ => None,
        })
        .collect();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].item["level"], "error");
    assert_eq!(
        notices[0].item["message"],
        "/btw requires interactive TUI mode"
    );
    assert!(matches!(
        events.last(),
        Some(AgentTurnEvent::Completed(None))
    ));
    session.close().await.unwrap();
    let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    assert!(!requests.contains("extension_ui_response"));
}

#[test]
fn notifications_preserve_levels_and_identity_without_changing_assistant_output() {
    let mut stream = streaming::Stream::default();
    stream.begin("turn".into());
    let mut keys = std::collections::BTreeSet::new();
    for (native, expected) in [
        ("info", "info"),
        ("warning", "warning"),
        ("error", "error"),
        ("future", "info"),
    ] {
        assert!(
            !stream
                .consume(&json!({"type":"extension_ui_request", "method":"notify",
            "id":"reused", "notifyType":native, "message":"Extension notice"}))
                .unwrap()
        );
        let AgentTurnEvent::Timeline(entry) = stream.pop().unwrap() else {
            panic!("expected notification timeline entry");
        };
        assert_eq!(entry.item["type"], "notification");
        assert_eq!(entry.item["level"], expected);
        assert_eq!(entry.item["metadata"]["notificationType"], native);
        assert_eq!(entry.item["metadata"]["nativeType"], "extension_ui_request");
        assert_eq!(entry.item["metadata"]["transient"], true);
        assert_eq!(entry.turn_id.as_deref(), Some("turn"));
        assert!(keys.insert(entry.key));
        assert!(stream.last_text.is_empty());
        assert!(stream.failure.is_none());
    }
}

#[test]
fn invalid_notification_metadata_is_bounded_without_failing_the_turn() {
    for notification_type in [
        json!("x".repeat(800 * 1024)),
        json!("x".repeat(129)),
        json!("é".repeat(65)),
        json!({"payload": "x".repeat(800 * 1024)}),
        json!(["x".repeat(800 * 1024)]),
        json!(42),
        json!(true),
        Value::Null,
    ] {
        let mut stream = streaming::Stream::default();
        stream.begin("turn".into());
        stream
            .consume(&json!({"type":"extension_ui_request", "method":"notify",
                "notifyType":notification_type, "message":"Extension notice"}))
            .unwrap();
        let AgentTurnEvent::Timeline(entry) = stream.pop().unwrap() else {
            panic!("expected notification timeline entry");
        };
        assert_eq!(entry.item["level"], "info");
        assert_eq!(entry.item["message"], "Extension notice");
        assert!(entry.item["metadata"]["notificationType"].is_null());
        assert!(serde_json::to_vec(&entry.item).unwrap().len() < 1024);

        stream
            .consume(&json!({"type":"message_end", "message":{"role":"assistant",
                "content":[{"type":"text", "text":"reply"}]}}))
            .unwrap();
        stream
            .consume(&json!({"type":"agent_settled", "aborted":false}))
            .unwrap();
        let events: Vec<_> = std::iter::from_fn(|| stream.pop()).collect();
        assert!(
            matches!(events.last(), Some(AgentTurnEvent::Completed(Some(text)))
            if text == "reply")
        );
    }
}

#[test]
fn notification_metadata_preserves_strings_at_the_byte_limit() {
    for notification_type in ["x".repeat(128), "é".repeat(64)] {
        let mut stream = streaming::Stream::default();
        stream.begin("turn".into());
        stream
            .consume(&json!({"type":"extension_ui_request", "method":"notify",
                "notifyType":notification_type, "message":"Extension notice"}))
            .unwrap();
        let AgentTurnEvent::Timeline(entry) = stream.pop().unwrap() else {
            panic!("expected notification timeline entry");
        };
        assert_eq!(entry.item["level"], "info");
        assert_eq!(
            entry.item["metadata"]["notificationType"],
            notification_type
        );
    }
}

#[test]
fn empty_notifications_are_ignored_and_notification_payloads_are_bounded() {
    let mut stream = streaming::Stream::default();
    stream.begin("turn".into());
    for message in [Value::Null, json!(""), json!(42)] {
        stream
            .consume(&json!({"type":"extension_ui_request", "method":"notify", "message":message}))
            .unwrap();
    }
    assert!(stream.pop().is_none());
    assert!(
        stream
            .consume(&json!({"type":"extension_ui_request", "method":"notify",
        "message":"x".repeat(96 * 1024 + 1)}))
            .is_ok()
    );
    assert!(stream.pop().is_none());
    let message = "x".repeat(96 * 1024);
    stream
        .consume(&json!({"type":"extension_ui_request", "method":"notify", "message":message}))
        .unwrap();
    let AgentTurnEvent::Timeline(entry) = stream.pop().unwrap() else {
        panic!("expected notification at the size limit");
    };
    assert_eq!(entry.item["message"], message);
}

#[test]
fn notice_flood_preserves_capacity_for_assistant_output() {
    let mut stream = streaming::Stream::default();
    stream.begin("turn".into());
    for _ in 0..300 {
        stream
            .consume(&json!({"type":"extension_ui_request", "method":"notify",
            "message":"x".repeat(96 * 1024)}))
            .unwrap();
    }
    for _ in 0..240 {
        stream
            .consume(&json!({"type":"message_update", "assistantMessageEvent":{
            "type":"text_delta", "contentIndex":0, "delta":"a"}}))
            .unwrap();
    }
    // Even with no display capacity left, a notice must be harmless.
    for _ in 0..14 {
        stream
            .consume(&json!({"type":"message_update", "assistantMessageEvent":{
            "type":"text_delta", "contentIndex":0, "delta":"a"}}))
            .unwrap();
    }
    stream
        .consume(&json!({"type":"extension_ui_request", "method":"notify", "message":"full"}))
        .unwrap();
    assert!(stream.failure.is_none());
    stream
        .consume(&json!({"type":"agent_settled", "aborted":false}))
        .unwrap();
    let mut events = Vec::new();
    while let Some(event) = stream.pop() {
        events.push(event);
    }
    assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
}

#[tokio::test]
async fn control_notice_flood_does_not_break_discovery_or_cancellation() {
    let (_directory, client, spec) = fixture("control-notices");
    assert_eq!(client.commands(&spec).await.unwrap().len(), 3);
    let mut session = client.create_session(&spec).await.unwrap();
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    session.cancel_turn(&turn).await.unwrap();
    let events = terminal(session.as_mut()).await;
    assert!(events.iter().any(|event| matches!(event,
        AgentTurnEvent::Timeline(entry) if entry.item["type"] == "notification"
            && entry.turn_id.as_deref() == Some(turn.as_str()))));
    assert!(matches!(events.last(), Some(AgentTurnEvent::Cancelled)));
    session.close().await.unwrap();
}

#[test]
fn notices_yield_reserved_capacity_to_tools() {
    let mut stream = streaming::Stream::default();
    stream.begin("turn".into());
    for _ in 0..16 {
        stream
            .consume(&json!({"type":"extension_ui_request", "method":"notify", "message":"notice"}))
            .unwrap();
    }
    for _ in 0..237 {
        stream
            .consume(&json!({"type":"message_update", "assistantMessageEvent":{
            "type":"text_delta", "contentIndex":0, "delta":"a"}}))
            .unwrap();
    }
    stream.consume(&json!({"type":"tool_execution_start", "toolCallId":"tool", "toolName":"read", "args":{}})).unwrap();
    assert!(stream.failure.is_none());
}

#[test]
fn submitted_input_follows_surviving_backlog_when_admission_discards_notices() {
    let mut stream = streaming::Stream::default();
    stream.begin("previous".into());
    stream.admission = true;
    for _ in 0..16 {
        stream
            .consume(&json!({"type":"extension_ui_request", "method":"notify", "message":"notice"}))
            .unwrap();
    }
    for index in 0..4078 {
        stream
            .consume(&json!({"type":"message_update", "assistantMessageEvent":{
            "type":"text_delta", "contentIndex":index, "delta":"old"}}))
            .unwrap();
    }
    assert_eq!(stream.events().len(), 4094);
    stream.begin("next".into());
    stream.admission = true;
    stream
        .consume(&json!({"type":"message_update", "assistantMessageEvent":{
        "type":"text_delta", "contentIndex":0, "delta":"new"}}))
        .unwrap();
    stream
        .insert_submitted(streaming::Stream::user_entries("next", None, "input", &[]))
        .unwrap();
    let events: Vec<_> = std::iter::from_fn(|| stream.pop()).collect();
    assert_eq!(events.len(), 4080);
    assert!(events[..4078].iter().all(|event| matches!(event,
        AgentTurnEvent::Progress { entry, .. } if entry.turn_id.as_deref() == Some("previous"))));
    assert!(matches!(&events[4078], AgentTurnEvent::Timeline(entry)
        if entry.item["type"] == "user_message"));
    assert!(
        matches!(&events[4079], AgentTurnEvent::Progress { entry, .. }
        if entry.item["text"] == "new")
    );
    // Draining and beginning another turn restores the notice allowance.
    stream.begin("third".into());
    for _ in 0..17 {
        stream
            .consume(&json!({"type":"extension_ui_request", "method":"notify",
            "message":"notice"}))
            .unwrap();
    }
    assert_eq!(std::iter::from_fn(|| stream.pop()).count(), 16);
}

#[tokio::test]
async fn idle_notice_flood_does_not_block_native_stdout() {
    let (directory, client, spec) = fixture("idle-notices");
    let mut transport = transport::Transport::spawn(&client, &spec.cwd, &[], None).unwrap();
    transport
        .request(json!({"type":"get_state"}))
        .await
        .unwrap();
    // Do not poll the transport until the peer has written far more than a pipe can hold.
    tokio::time::timeout(Duration::from_secs(5), async {
        while !directory.path().join("notices-written").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("idle notice flood blocked native stdout");
    assert!(transport.request(json!({"type":"get_state"})).await.is_ok());
    transport.close().await.unwrap();
}

#[test]
fn earlier_turn_notices_yield_allowance_without_displacing_other_output() {
    let mut stream = streaming::Stream::default();
    let notice = json!({"type":"extension_ui_request", "method":"notify", "message":"notice"});
    stream.begin("previous".into());
    stream
        .consume(&json!({"type":"message_update", "assistantMessageEvent":{
        "type":"text_delta", "contentIndex":0, "delta":"old output"}}))
        .unwrap();
    for _ in 0..16 {
        stream.consume(&notice).unwrap();
    }
    stream.begin("next".into());
    // Earlier notices remain available for publication until their capacity is needed.
    assert_eq!(stream.events().len(), 17);
    for _ in 0..17 {
        stream.consume(&notice).unwrap();
    }
    stream
        .insert_submitted(streaming::Stream::user_entries("next", None, "input", &[]))
        .unwrap();
    assert!(matches!(
        stream.pop(),
        Some(AgentTurnEvent::Progress { .. })
    ));
    assert!(matches!(stream.pop(), Some(AgentTurnEvent::Timeline(entry))
        if entry.item["type"] == "user_message"));
    for _ in 0..16 {
        assert!(matches!(stream.pop(), Some(AgentTurnEvent::Timeline(entry))
            if entry.item["type"] == "notification" && entry.turn_id.as_deref() == Some("next")));
    }
    assert!(stream.pop().is_none());
    stream.consume(&notice).unwrap();
    assert!(stream.pop().is_some());
}

#[test]
fn draining_earlier_notices_does_not_release_current_turn_allowance() {
    let mut stream = streaming::Stream::default();
    let notice = json!({"type":"extension_ui_request", "method":"notify", "message":"notice"});
    stream.begin("previous".into());
    stream.consume(&notice).unwrap();
    stream.begin("next".into());
    stream.consume(&notice).unwrap();
    assert!(matches!(stream.pop(), Some(AgentTurnEvent::Timeline(entry))
        if entry.turn_id.as_deref() == Some("previous")));
    for _ in 0..16 {
        stream.consume(&notice).unwrap();
    }
    assert_eq!(std::iter::from_fn(|| stream.pop()).count(), 16);
}

#[test]
fn notifications_strip_terminal_formatting_and_clean_control_characters() {
    let mut stream = streaming::Stream::default();
    stream.begin("turn".into());
    for (message, expected) in [
        ("Plain é text", "Plain é text"),
        (
            "\u{1b}[38;2;135;144;149mColored\u{1b}[0m notice",
            "Colored notice",
        ),
        (
            "\u{1b}]8;;https://example.com\u{7}Link\u{1b}]8;;\u{1b}\\",
            "Link",
        ),
        ("Line\nnext\r\tword\0\u{85}end", "Line next  word  end"),
        ("\u{9b}31mRed\u{9b}0m", "Red"),
    ] {
        stream
            .consume(&json!({"type":"extension_ui_request", "method":"notify",
            "message":message}))
            .unwrap();
        let Some(AgentTurnEvent::Timeline(entry)) = stream.pop() else {
            panic!("expected notification");
        };
        assert_eq!(entry.item["message"], expected);
    }
    stream
        .consume(&json!({"type":"extension_ui_request", "method":"notify",
        "message":"\u{1b}[31m\u{1b}[0m\0"}))
        .unwrap();
    assert!(stream.pop().is_none());
}

#[tokio::test]
async fn handled_readback_preserves_notice_after_submitted_input() {
    let (_directory, client, spec) = fixture("");
    let mut session = client.create_session(&spec).await.unwrap();
    for _ in 0..2 {
        let turn = session
            .start_turn("handled-control-notice", &spec.config)
            .await
            .unwrap();
        let events = terminal(session.as_mut()).await;
        let rows: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                AgentTurnEvent::Timeline(entry) => Some(entry),
                _ => None,
            })
            .collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].item["type"], "user_message");
        assert_eq!(rows[1].item["type"], "notification");
        assert_eq!(rows[1].item["message"], "handled notice");
        assert_eq!(rows[1].turn_id.as_deref(), Some(turn.as_str()));
        assert!(matches!(
            events.last(),
            Some(AgentTurnEvent::Completed(None))
        ));
    }
    session.close().await.unwrap();
}

#[test]
fn restarting_same_turn_preserves_notice_allowance_and_drains_cleanly() {
    let mut stream = streaming::Stream::default();
    let notice = json!({"type":"extension_ui_request", "method":"notify", "message":"notice"});
    stream.begin("turn".into());
    for _ in 0..8 {
        stream.consume(&notice).unwrap();
    }
    stream.begin("turn".into());
    for _ in 0..16 {
        stream.consume(&notice).unwrap();
    }
    assert_eq!(std::iter::from_fn(|| stream.pop()).count(), 16);
    stream.consume(&notice).unwrap();
    assert!(stream.pop().is_some());
    assert!(stream.pop().is_none());
}

#[test]
fn idle_notices_never_enter_the_timeline() {
    let mut stream = streaming::Stream::default();
    let notice = json!({"type":"extension_ui_request", "method":"notify", "message":"idle"});
    stream.consume(&notice).unwrap();
    assert!(stream.pop().is_none());
    for terminal in ["completed", "cancelled", "handled", "failed"] {
        stream.begin(terminal.into());
        match terminal {
            "handled" => stream.handled().unwrap(),
            "failed" => stream.fail(),
            _ => {
                stream
                    .consume(&json!({"type":"agent_settled", "aborted":terminal == "cancelled"}))
                    .unwrap();
            }
        }
        stream.consume(&notice).unwrap();
        while let Some(event) = stream.pop() {
            assert!(
                !matches!(event, AgentTurnEvent::Timeline(entry) if entry.item["type"] == "notification")
            );
        }
    }
    stream.begin("next".into());
    assert!(stream.pop().is_none());
    stream.consume(&notice).unwrap();
    assert!(
        matches!(stream.pop(), Some(AgentTurnEvent::Timeline(entry)) if entry.turn_id.as_deref() == Some("next"))
    );
}
