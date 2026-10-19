use super::*;
use projection::Projection;

mod http;

fn catalog() -> Value {
    json!({"providers":[{"id":"local","models":{"model":{"limit":{"context":1_000_000}}}}]})
}

fn message(id: &str, created: u64, input: u64, cached: u64) -> Value {
    json!({"type":"message.updated","properties":{"info":{"id":id,"sessionID":"parent",
        "role":"assistant","time":{"created":created},"providerID":"local","modelID":"model",
        "tokens":{"input":input,"cache":{"read":cached}}}}})
}

#[test]
fn native_usage_counts_cached_input_and_keeps_the_last_real_request_across_placeholders() {
    let mut projection = Projection::default();
    projection.catalog(&catalog()).unwrap();
    let updates = projection
        .event("parent", &message("step-1", 1, 251, 629_632))
        .unwrap();
    assert_eq!(updates[0]["used"], 629_883);
    assert_eq!(updates[0]["size"], 1_000_000);
    assert_eq!(
        projection
            .event("parent", &message("placeholder", 2, 0, 0))
            .unwrap(),
        [] as [Value; 0]
    );
    assert_eq!(
        projection
            .event("parent", &message("old", 0, 100, 0))
            .unwrap(),
        [] as [Value; 0]
    );
    assert_eq!(
        projection
            .event("child", &message("foreign", 3, 500, 0))
            .unwrap(),
        [] as [Value; 0]
    );
    let updates = projection
        .event("parent", &message("after-summary", 3, 12_000, 0))
        .unwrap();
    assert_eq!(updates[0]["used"], 12_000);
}

#[test]
fn invalid_native_token_counts_cannot_replace_valid_usage_or_fail_acp() {
    let mut projection = Projection::default();
    projection.catalog(&catalog()).unwrap();
    projection
        .event("parent", &message("real", 1, 100, 0))
        .unwrap();
    for input in [9_007_199_254_740_992, u64::MAX] {
        assert_eq!(
            projection
                .event("parent", &message("invalid", 2, input, 1))
                .unwrap(),
            [] as [Value; 0]
        );
        assert_eq!(projection.usage.as_ref().unwrap()["used"], 100);
    }
    projection.limits.insert("local/model".into(), u64::MAX);
    assert_eq!(
        projection
            .event("parent", &message("bad-limit", 3, 200, 0))
            .unwrap(),
        [] as [Value; 0]
    );
    assert_eq!(projection.usage.as_ref().unwrap()["used"], 100);
}

#[test]
fn native_compaction_lifecycle_and_replay_share_the_same_acp_identity_and_turn() {
    for terminal in ["completed", "failed"] {
        let mut projection = Projection::default();
        let started = json!({"type":"message.part.updated","properties":{"part":{
            "type":"compaction","messageID":"compact","sessionID":"parent","auto":true}}});
        let updates = projection.event("parent", &started).unwrap();
        assert_eq!(
            updates[0]["_meta"]["opencode/compaction"]["status"],
            "started"
        );
        assert_eq!(
            projection.event("parent", &started).unwrap(),
            [] as [Value; 0]
        );
        let mut ended = message("summary", 2, 0, 0);
        let info = &mut ended["properties"]["info"];
        info["summary"] = json!(true);
        info["parentID"] = json!("compact");
        info["finish"] = json!("stop");
        if terminal == "failed" {
            info["error"] = json!({"message":"private diagnostic"});
        }
        let updates = projection.event("parent", &ended).unwrap();
        assert_eq!(
            updates[0]["_meta"]["opencode/compaction"]["status"],
            terminal
        );
        assert!(!format!("{updates:?}").contains("private diagnostic"));
        let mut entries = vec![NativeItem {
            key: "text".into(),
            turn_id: Some("accepted".into()),
            timestamp: "2026-10-19T00:00:00Z".into(),
            item: json!({"type":"assistant_message",
            "messageId":"summary","text":"Native summary"}),
        }];
        projection.history(&mut entries, &std::collections::BTreeMap::new());
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].item["status"], terminal);
        assert_eq!(entries[0].turn_id.as_deref(), Some("accepted"));
        projection.history(&mut entries, &std::collections::BTreeMap::new());
        assert_eq!(entries.len(), 2);
    }
}

#[test]
fn restoring_metadata_does_not_invent_a_historical_compaction_start() {
    let mut projection = Projection::default();
    let rows = serde_json::from_value::<Vec<projection::Row>>(json!([
        {"info":{"id":"compact","role":"user"},"parts":[{"type":"compaction","auto":false}]},
        {"info":{"id":"summary","role":"assistant","summary":true,"parentID":"compact",
            "finish":"stop","time":{"created":1}},"parts":[]}
    ]))
    .unwrap();
    let updates = projection.snapshot(&rows).unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(
        updates[0]["_meta"]["opencode/compaction"]["status"],
        "completed"
    );
}

#[test]
fn unknown_native_finish_is_scoped_to_new_assistant_messages_and_cost_is_not_accumulated() {
    let mut projection = Projection::default();
    projection.catalog(&catalog()).unwrap();
    let mut ended = message("old", 1, 20, 0);
    ended["properties"]["info"]["finish"] = json!("unknown");
    projection.event("parent", &ended).unwrap();
    assert!(projection.unknown_finish());
    projection.begin();
    assert!(!projection.unknown_finish());
    ended["properties"]["info"]["id"] = json!("new");
    ended["properties"]["info"]["time"]["created"] = json!(2);
    projection.event("parent", &ended).unwrap();
    assert!(projection.unknown_finish());
    for _ in 0..2 {
        projection.session(&json!({"cost":0.02}));
    }
    assert_eq!(projection.usage.unwrap()["cost"]["amount"], 0.02);
}

#[test]
fn native_source_is_only_enabled_for_the_validated_legacy_profile() {
    assert!(
        Source::prepare("2.0.25", Duration::from_secs(1))
            .unwrap()
            .is_none()
    );
    assert!(
        Source::prepare("1.17.0", Duration::from_secs(1))
            .unwrap()
            .is_none()
    );
    let source = Source::prepare("1.18.4", Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert!(!format!("{source:?}").contains("Basic "));
    assert!(relevant(&message("message", 1, 1, 0), "parent"));
    assert!(!relevant(&message("message", 1, 1, 0), "child"));
}

#[test]
fn interrupted_empty_summaries_stay_in_their_own_turn_before_the_followup_input() {
    let mut projection = Projection::default();
    let rows = serde_json::from_value::<Vec<projection::Row>>(json!([
        {"info":{"id":"compact","role":"user","time":{"created":1}},
            "parts":[{"type":"compaction","auto":false}]},
        {"info":{"id":"summary","role":"assistant","summary":true,"parentID":"compact",
            "error":{"name":"MessageAbortedError"},"time":{"created":2}},"parts":[]},
        {"info":{"id":"followup","role":"user","time":{"created":3}},"parts":[{"type":"text"}]}
    ]))
    .unwrap();
    projection.snapshot(&rows).unwrap();
    let mut entries = vec![NativeItem {
        key: "followup".into(),
        turn_id: Some("next-turn".into()),
        timestamp: "2026-10-19T00:00:00Z".into(),
        item: json!({"type":"user_message","messageId":"followup"}),
    }];
    projection.history(
        &mut entries,
        &std::collections::BTreeMap::from([("compact".into(), "compact-turn".into())]),
    );
    assert_eq!(entries[0].item["status"], "failed");
    assert_eq!(entries[0].turn_id.as_deref(), Some("compact-turn"));
    assert_eq!(entries[1].turn_id.as_deref(), Some("next-turn"));
}

#[test]
fn native_completion_fails_an_unfinished_compaction_instead_of_leaving_it_loading() {
    let mut projection = Projection::default();
    projection
        .event(
            "parent",
            &json!({"type":"message.part.updated","properties":{"part":{
        "type":"compaction","messageID":"compact","sessionID":"parent","auto":false}}}),
        )
        .unwrap();
    assert_eq!(projection.users, BTreeSet::new());
    let updates = projection.finish().unwrap();
    assert_eq!(
        updates[0]["_meta"]["opencode/compaction"]["status"],
        "failed"
    );
    assert_eq!(projection.finish().unwrap(), [] as [Value; 0]);
}
