use super::*;

#[test]
fn resumed_settlement_discards_prior_failure_and_text() {
    for stop_reason in ["error", "stop"] {
        for native_start in [false, true] {
            let mut stream = streaming::Stream::default();
            stream.begin("turn".into());
            stream
                .consume(&json!({"type":"message_end","message":{
                    "role":"assistant","stopReason":stop_reason,"errorMessage":"quota exceeded",
                    "content":[{"type":"text","text":"previous run"}]
                }}))
                .unwrap();
            stream
                .consume(&json!({"type":"agent_settled","aborted":false}))
                .unwrap();

            if native_start {
                stream.consume(&json!({"type":"agent_start"})).unwrap();
            } else {
                stream.resume_running();
            }
            assert!(stream.running);
            assert!(!stream.settled);
            assert!(stream.failure.is_none());
            assert_eq!(stream.last_text, "");
            assert_eq!(stream.events.len(), 1);
            assert!(
                matches!(stream.events.front(), Some(AgentTurnEvent::Timeline(entry))
                if entry.item["text"] == "previous run")
            );

            stream
                .consume(&json!({"type":"agent_settled","aborted":false}))
                .unwrap();
            assert!(
                matches!(stream.events.back(), Some(AgentTurnEvent::Completed(Some(text)))
                if text.is_empty())
            );
        }
    }
}

#[test]
fn resuming_running_work_preserves_current_failure_and_text() {
    let mut stream = streaming::Stream::default();
    stream.begin("turn".into());
    stream.consume(&json!({"type":"agent_start"})).unwrap();
    stream
        .consume(&json!({"type":"message_end","message":{
            "role":"assistant","stopReason":"error","errorMessage":"quota exceeded",
            "content":[{"type":"text","text":"current run"}]
        }}))
        .unwrap();

    stream.resume_running();

    assert!(stream.failure.is_some());
    assert_eq!(stream.last_text, "current run");
    stream
        .consume(&json!({"type":"agent_settled","aborted":false}))
        .unwrap();
    assert!(matches!(stream.events.back(), Some(AgentTurnEvent::Failed)));
}

#[test]
fn admission_coalesces_deltas_and_final_items_without_losing_text() {
    let mut stream = streaming::Stream::default();
    stream.begin("turn".into());
    stream.admission = true;
    let delta = json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":"é"}});
    for _ in 0..5000 {
        stream.consume(&delta).unwrap();
    }
    assert_eq!(stream.events.len(), 1);
    assert!(
        matches!(stream.events.front(), Some(AgentTurnEvent::Progress { entry, .. }) if entry.item["text"] == "é".repeat(5000))
    );
    stream.consume(&json!({"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"authoritative"}]}})).unwrap();
    assert_eq!(stream.events.len(), 1);
    assert!(
        matches!(stream.events.front(), Some(AgentTurnEvent::Timeline(entry)) if entry.item["text"] == "authoritative")
    );
}

#[test]
fn admission_tool_snapshots_coalesce_to_distinct_completed_items() {
    let mut stream = streaming::Stream::default();
    stream.begin("turn".into());
    stream.admission = true;
    for id in 0..1400 {
        let mut event = json!({"type":"tool_execution_start","toolCallId":id.to_string(),"toolName":"bash","args":{}});
        stream.consume(&event).unwrap();
        event["type"] = json!("tool_execution_update");
        event["partialResult"] = json!("latest output");
        stream.consume(&event).unwrap();
        assert!(
            matches!(stream.events.back(), Some(AgentTurnEvent::Progress { entry, .. }) if entry.item["detail"]["output"] == "latest output")
        );
        event["type"] = json!("tool_execution_end");
        event["result"] = json!("final output");
        stream.consume(&event).unwrap();
    }
    assert_eq!(stream.events.len(), 1400);
    assert!(
        stream
            .events
            .iter()
            .all(|event| matches!(event, AgentTurnEvent::Timeline(_)))
    );
    stream.end_admission();
    while stream.pop().is_some() {}
    let delta = json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":"x"}});
    assert!((0..256).any(|_| stream.consume(&delta).is_err()));
}

#[test]
fn queued_tool_previews_remain_bounded_and_release_capacity_when_drained() {
    for admission in [false, true] {
        let mut stream = streaming::Stream::default();
        stream.begin("turn".into());
        stream.admission = admission;
        let mut completed = 0;
        loop {
            let id = completed.to_string();
            let start = json!({"type":"tool_execution_start","toolCallId":id,"toolName":"bash","args":"x".repeat(32*1024)});
            let end = json!({"type":"tool_execution_end","toolCallId":id,"toolName":"bash","result":"y".repeat(32*1024)});
            if stream
                .consume(&start)
                .and_then(|_| stream.consume(&end))
                .is_err()
            {
                break;
            }
            completed += 1;
            assert!(completed <= 1024);
        }
        assert_eq!(completed, if admission { 1024 } else { 128 });
        assert_eq!(stream.failure.unwrap().source, "display_limit");
        stream.fail();
        while stream.pop().is_some() {}
        stream.begin("next".into());
        stream.consume(&json!({"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}})).unwrap();
        assert!(stream.failure.is_none());
    }
}

#[test]
fn long_tools_replace_pending_previews_and_drained_calls_do_not_accumulate_bytes() {
    for admission in [false, true] {
        let mut stream = streaming::Stream::default();
        stream.begin("turn".into());
        stream.admission = admission;
        stream.consume(&json!({"type":"tool_execution_start","toolCallId":"streaming","toolName":"bash","args":"x".repeat(32*1024)})).unwrap();
        let update = json!({"type":"tool_execution_update","toolCallId":"streaming","toolName":"bash","partialResult":"y".repeat(32*1024)});
        for _ in 0..3000 {
            stream.consume(&update).unwrap();
        }
        assert_eq!(stream.events.len(), 1);
        stream.consume(&json!({"type":"tool_execution_end","toolCallId":"streaming","toolName":"bash","result":"final"})).unwrap();
        assert_eq!(stream.events.len(), 1);
        while stream.pop().is_some() {}
        for id in 0..1000 {
            stream.consume(&json!({"type":"tool_execution_start","toolCallId":id.to_string(),"toolName":"bash","args":"x".repeat(32*1024)})).unwrap();
            while stream.pop().is_some() {}
            stream.consume(&json!({"type":"tool_execution_end","toolCallId":id.to_string(),"toolName":"bash","result":"y".repeat(32*1024)})).unwrap();
            while stream.pop().is_some() {}
        }
        stream
            .consume(&json!({"type":"agent_settled","aborted":false}))
            .unwrap();
        assert!(matches!(stream.pop(), Some(AgentTurnEvent::Completed(_))));
        assert!(stream.failure.is_none());
    }
}

#[tokio::test]
async fn cancellation_preserves_undrained_admission_items_and_session_reuse() {
    let (_directory, client, spec) = fixture("");
    let mut session = client.create_session(&spec).await.unwrap();
    let turn = session
        .start_turn("handled-backlog", &spec.config)
        .await
        .unwrap();
    // Do not poll: all 300 completed tools and one running tool remain queued.
    session.cancel_turn(&turn).await.unwrap();
    let events = terminal(session.as_mut()).await;
    assert!(matches!(events.last(), Some(AgentTurnEvent::Cancelled)));
    assert_eq!(events.iter().filter(|event| matches!(event, AgentTurnEvent::Timeline(entry) if entry.item["type"] == "tool_call")).count(), 301);
    assert!(session.failure_diagnostic().is_none());
    session.start_turn("hello", &spec.config).await.unwrap();
    assert!(
        matches!(terminal(session.as_mut()).await.last(), Some(AgentTurnEvent::Completed(Some(text))) if text == "Hello")
    );
    session.close().await.unwrap();
}

#[test]
fn duplicate_queued_tool_identity_cannot_overwrite_byte_accounting() {
    let mut stream = streaming::Stream::default();
    stream.begin("turn".into());
    let start =
        json!({"type":"tool_execution_start","toolCallId":"same","toolName":"bash","args":{}});
    stream.consume(&start).unwrap();
    stream.consume(&json!({"type":"tool_execution_end","toolCallId":"same","toolName":"bash","result":"done"})).unwrap();
    assert!(stream.consume(&start).is_err());
    assert_eq!(stream.events.len(), 1);
}
