use super::*;

mod command_actions;
mod file_changes;
mod paseo;

#[test]
fn compaction_start_is_visible_once_and_stops_after_completion() {
    let mut stream = Stream::default();
    let native = json!({"type":"contextCompaction","id":"compact"});
    let params = json!({"turnId":"turn","item":native});
    let Some(AgentTurnEvent::Timeline(started)) = stream.progress("item/started", &params).unwrap()
    else {
        panic!("compaction must emit a durable loading marker");
    };
    assert_eq!(started.key, "native:turn:compact:loading");
    assert_eq!(started.turn_id.as_deref(), Some("turn"));
    assert_eq!(
        started.item,
        json!({"type":"compaction","status":"loading"})
    );
    assert!(stream.progress("item/started", &params).unwrap().is_none());

    assert!(stream.complete(&native).unwrap());
    let completed = discovery::timeline_item(&native, "turn", &started.timestamp)
        .unwrap()
        .unwrap();
    assert_eq!(completed.item["status"], "completed");
    let timeline = crate::storage::timeline::Timeline::memory().unwrap();
    timeline.append("agent", "codex", &[started]).unwrap();
    timeline.append("agent", "codex", &[completed]).unwrap();
    assert_eq!(timeline.read("agent").unwrap().1.len(), 2);
    assert!(stream.compactions.is_empty());
    assert!(stream.progress("item/started", &params).unwrap().is_none());
}

#[test]
fn compaction_start_rejects_missing_identity_and_excessive_items() {
    let mut stream = Stream::default();
    for params in [
        json!({"turnId":"turn","item":{"type":"contextCompaction"}}),
        json!({"item":{"type":"contextCompaction","id":"compact"}}),
    ] {
        assert!(stream.progress("item/started", &params).is_err());
    }
    stream.compactions = (0..MAX_ITEMS).map(|id| id.to_string()).collect();
    let params = json!({"turnId":"turn","item":{"type":"contextCompaction","id":"extra"}});
    assert!(stream.progress("item/started", &params).is_err());
}

#[test]
fn only_known_steer_errors_prove_that_input_was_not_accepted() {
    for error in [
        json!({"code":-32601}),
        json!({"code":-32600,"message":"no active turn to steer"}),
        json!({"code":-32600,"message":"active turn uses a different output schema"}),
        json!({"code":-32600,"message":"expected active turn id `first` but found `second`"}),
        json!({"code":-32600,"data":{"codexErrorInfo":{"activeTurnNotSteerable":{}}}}),
    ] {
        assert!(steer_rejected(&error), "{error}");
    }
    for error in [
        json!({"code":-32603,"message":"no active turn to steer"}),
        json!({"code":-32600,"message":"unexpected invalid request"}),
        json!({"code":-32600}),
        json!({"message":"no active turn to steer"}),
        json!({"code":-32600,"message":"expected active turn id `` but found `second`"}),
        json!({"code":-32600,"message":"expected active turn id `one` but found `second` extra"}),
    ] {
        assert!(!steer_rejected(&error), "{error}");
    }
}

fn params(id: &str, delta: &str) -> Value {
    json!({"threadId":"thread","turnId":"turn","itemId":id,"delta":delta,"summaryIndex":0})
}

fn item(event: Option<AgentTurnEvent>) -> NativeItem {
    match event.unwrap() {
        AgentTurnEvent::Progress { entry, .. } => entry,
        other => panic!("{other:?}"),
    }
}

#[test]
fn text_deltas_preserve_identity_unicode_and_reasoning_boundaries() {
    let mut stream = Stream::default();
    let first = item(
        stream
            .progress("item/agentMessage/delta", &params("a", "你好"))
            .unwrap(),
    );
    assert_eq!(first.key, "native:turn:a");
    assert_eq!(
        first.item,
        json!({"type":"assistant_message","messageId":"a","text":"你好"})
    );
    assert!(
        stream
            .progress("item/agentMessage/delta", &params("a", ""))
            .unwrap()
            .is_none()
    );
    item(
        stream
            .progress("item/reasoning/summaryTextDelta", &params("r", "first"))
            .unwrap(),
    );
    let mut next = params("r", "second");
    next["summaryIndex"] = json!(1);
    assert_eq!(
        item(
            stream
                .progress("item/reasoning/summaryTextDelta", &next)
                .unwrap()
        )
        .item["text"],
        "\nsecond"
    );
    next["summaryIndex"] = json!(0);
    assert!(
        stream
            .progress("item/reasoning/summaryTextDelta", &next)
            .is_err()
    );
    assert!(stream.complete(&json!({"id":"a"})).unwrap());
    assert!(!stream.complete(&json!({"id":"a"})).unwrap());
    assert!(
        stream
            .progress("item/agentMessage/delta", &params("a", "late"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn tool_previews_are_bounded_and_stop_after_completion() {
    let mut stream = Stream::default();
    let started =
        json!({"turnId":"turn","item":{"type":"commandExecution","id":"tool","command":"echo"}});
    assert_eq!(
        item(stream.progress("item/started", &started).unwrap()).item["status"],
        "running"
    );
    assert!(stream.progress("item/started", &started).unwrap().is_none());
    let first = item(
        stream
            .progress("item/commandExecution/outputDelta", &params("tool", "one"))
            .unwrap(),
    );
    assert_eq!(first.item["detail"]["output"], "one");
    let last = item(
        stream
            .progress(
                "item/fileChange/outputDelta",
                &params("tool", &"界".repeat(MAX_OUTPUT)),
            )
            .unwrap(),
    );
    assert!(last.item["detail"]["output"].as_str().unwrap().len() <= MAX_OUTPUT);
    stream.complete(&json!({"id":"tool"})).unwrap();
    assert!(
        stream
            .progress("item/fileChange/outputDelta", &params("tool", "late"))
            .unwrap()
            .is_none()
    );
    assert!(stream.progress("ignored", &json!({})).unwrap().is_none());
}

#[test]
fn malformed_and_excessive_progress_is_rejected() {
    let mut stream = Stream::default();
    assert!(
        stream
            .progress("item/agentMessage/delta", &json!({}))
            .is_err()
    );
    assert!(
        stream
            .progress(
                "item/agentMessage/delta",
                &params("a", &"x".repeat(MAX_TEXT + 1))
            )
            .is_err()
    );
    assert!(stream.complete(&json!({})).is_err());
    for index in 0..MAX_ITEMS {
        stream.complete(&json!({"id":index.to_string()})).unwrap();
    }
    assert!(stream.complete(&json!({"id":"overflow"})).is_err());
}
