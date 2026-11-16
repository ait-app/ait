use super::*;

fn segment(id: &str, role: &str, text: &str) -> Value {
    json!({"id":id,"realtimeSessionId":"call","type":"transcriptSegment","role":role,"text":text})
}

#[test]
fn both_roles_stream_snapshots_and_commit_corrected_text_once() {
    let mut timeline = Timeline::default();
    for role in ["user", "assistant"] {
        let native = segment(role, role, "");
        let started = timeline
            .observe("thread/realtime/item/started", &json!({"item":native}))
            .unwrap()
            .unwrap();
        let AgentTurnEvent::Progress { entry: first, .. } = started else {
            panic!("progress")
        };
        for delta in ["hello", " world"] {
            let event = timeline
                .observe(
                    "thread/realtime/item/transcript/delta",
                    &json!({"itemId":role,"delta":delta}),
                )
                .unwrap()
                .unwrap();
            let AgentTurnEvent::Progress { entry, .. } = event else {
                panic!("progress")
            };
            assert_eq!(entry.key, first.key);
            assert_eq!(entry.item["role"], role);
        }
        let final_item = json!({"item":segment(role, role, "Hello, world!")});
        let event = timeline
            .observe("thread/realtime/item/completed", &final_item)
            .unwrap()
            .unwrap();
        let AgentTurnEvent::Timeline(entry) = event else {
            panic!("completed")
        };
        assert_eq!(entry.key, first.key);
        assert_eq!(entry.timestamp, first.timestamp);
        assert_eq!(entry.turn_id, None);
        assert_eq!(entry.item["text"], "Hello, world!");
        assert_eq!(entry.item["status"], "completed");
        assert!(
            timeline
                .observe("thread/realtime/item/completed", &final_item)
                .unwrap()
                .is_none()
        );
        assert!(
            timeline
                .observe(
                    "thread/realtime/item/transcript/delta",
                    &json!({"itemId":role,"delta":"late"})
                )
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn unrecognized_or_oversized_speech_never_invents_identity() {
    let mut timeline = Timeline::default();
    assert!(
        timeline
            .observe(
                "thread/realtime/item/transcript/delta",
                &json!({"itemId":"unknown","delta":"x"})
            )
            .unwrap()
            .is_none()
    );
    assert!(project(&segment("id", "developer", "x"), "time").is_err());
    assert!(project(&segment("id", "user", &"x".repeat(MAX_TEXT + 1)), "time").is_err());
    assert!(
        project(&json!({"type":"futureType"}), "time")
            .unwrap()
            .is_none()
    );
}

#[test]
fn restarted_call_can_reuse_an_item_id_without_old_completion_consuming_the_new_partial() {
    let mut timeline = Timeline::default();
    let old = segment("same", "user", "old");
    timeline
        .observe("thread/realtime/item/started", &json!({"item":old}))
        .unwrap();
    let mut new = segment("same", "user", "new");
    new["realtimeSessionId"] = json!("next-call");
    timeline
        .observe("thread/realtime/item/started", &json!({"item":new}))
        .unwrap();
    timeline
        .observe("thread/realtime/item/completed", &json!({"item":old}))
        .unwrap();
    let Some(AgentTurnEvent::Progress { entry, .. }) = timeline
        .observe(
            "thread/realtime/item/transcript/delta",
            &json!({"itemId":"same","delta":" text"}),
        )
        .unwrap()
    else {
        panic!("new call progress")
    };
    assert_eq!(entry.item["sessionId"], "next-call");
    assert_eq!(entry.item["text"], "new text");
}

#[test]
fn handoff_dedup_preserves_new_input_and_unrepresented_excerpts() {
    let mut timeline = Timeline::default();
    timeline
        .remember(
            &project(&segment("one", "user", "hello"), "time")
                .unwrap()
                .unwrap(),
        )
        .unwrap();
    let mut item = NativeItem {
        key: "native:turn:input".into(),
        turn_id: Some("turn".into()),
        timestamp: "time".into(),
        item: json!({"type":"user_message","text":"hello","voiceTranscript":[{"role":"user","text":"hello"}]}),
    };
    assert!(!timeline.retain_handoff(&mut item));
    item.item["text"] = json!("different instruction");
    assert!(timeline.retain_handoff(&mut item));
    assert!(item.item.get("voiceTranscript").is_none());
    item.item["voiceTranscript"] =
        json!([{"role":"user","text":"hello"},{"role":"assistant","text":"unrepresented"}]);
    assert!(timeline.retain_handoff(&mut item));
    assert_eq!(
        item.item["voiceTranscript"],
        json!([{"role":"assistant","text":"unrepresented"},{"role":"user","text":"different instruction"}])
    );
}

#[test]
fn boundaries_and_promotions_keep_native_identity() {
    let mut timeline = Timeline::default();
    for (kind, outcome, expected) in [
        ("realtimeSessionStarted", "", "started"),
        ("realtimeSessionClosed", "ended", "ended"),
        ("realtimeSessionClosed", "failed", "failed"),
    ] {
        let entry = project(
            &json!({"type":kind,"id":expected,"realtimeSessionId":"call","outcome":outcome}),
            "time",
        )
        .unwrap()
        .unwrap();
        assert_eq!(entry.item["status"], expected);
    }
    for presentation in [
        json!({"type":"wholeItem"}),
        json!({"type":"inlineMarkdown"}),
        json!({"type":"inlineVisualization","index":0}),
    ] {
        timeline
            .promote(&json!({"turnId":"turn","itemId":"message","presentation":presentation}))
            .unwrap();
    }
    assert_eq!(timeline.promoted.len(), 1);
}
