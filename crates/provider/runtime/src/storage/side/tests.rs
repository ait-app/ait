use super::*;
use provider_core::protocol::side_questions::{BtwHistoryTurn, SideTurnStatus};

fn snapshot() -> SideContextSnapshot {
    SideContextSnapshot {
        v: 1,
        agent_id: "agent".to_owned(),
        provider: "claude".to_owned(),
        model: Some("selected-model".to_owned()),
        native_session_id: None,
        native_leaf_id: None,
        captured_at: 1,
        entries: Vec::new(),
        truncated: false,
    }
}

fn topic() -> SideTopic {
    serde_json::from_value(serde_json::json!({
        "id":"topic", "leafId":null, "question":"q", "answer":"answer",
        "status":"complete", "createdAt":1, "updatedAt":2,
        "strategy":"isolated", "truncated":false
    }))
    .unwrap()
}

#[test]
fn accepted_replay_and_conflicting_reuse_cannot_admit_another_send() {
    let store = SideStore::memory().unwrap();
    assert_eq!(
        store.claim_ask("agent", "r1", "q", None).unwrap(),
        AskClaim::New
    );
    store
        .prepare_ask("agent", "r1", SideStrategy::Isolated, &snapshot())
        .unwrap();
    store
        .settle_ask(
            "agent",
            "r1",
            &SideAskResponse {
                receipt: SideReceiptState::Accepted,
                record_id: Some("topic".to_owned()),
                strategy: Some(SideStrategy::Isolated),
                reason: None,
            },
            2,
        )
        .unwrap();
    assert!(matches!(
        store.claim_ask("agent", "r1", "q", None).unwrap(),
        AskClaim::Accepted(_)
    ));
    assert_eq!(
        store.claim_ask("agent", "r1", "changed", None).unwrap(),
        AskClaim::Conflict
    );
}

#[test]
fn crash_after_claim_is_uncertain_and_confirmed_cleanup_never_replays_it() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("side.sqlite");
    {
        let store = SideStore::open(&path).unwrap();
        assert_eq!(
            store.claim_ask("agent", "lost-ack", "q", None).unwrap(),
            AskClaim::New
        );
        store
            .prepare_ask("agent", "lost-ack", SideStrategy::Isolated, &snapshot())
            .unwrap();
    }
    let store = SideStore::open(&path).unwrap();
    assert!(matches!(
        store.claim_ask("agent", "lost-ack", "q", None).unwrap(),
        AskClaim::Uncertain(_)
    ));
    assert!(store.uncertain_active("agent", "new-request").unwrap());
    assert_eq!(store.pending("agent").unwrap()[0].1, Some(snapshot()));
    store.release_uncertain("agent", "lost-ack").unwrap();
    assert!(!store.uncertain_active("agent", "new-request").unwrap());
    assert!(matches!(
        store.claim_ask("agent", "lost-ack", "q", None).unwrap(),
        AskClaim::Uncertain(_)
    ));
}

#[test]
fn followup_preserves_original_snapshot_and_stale_root_cannot_overwrite_it() {
    let store = SideStore::memory().unwrap();
    let first = topic();
    let frozen = snapshot();
    store.insert_topic("agent", first.clone(), &frozen).unwrap();
    let mut followup = first.clone();
    followup.record.follow_ups.push(BtwHistoryTurn {
        question: "followup".to_owned(),
        images: Vec::new(),
        answer: "partial".to_owned(),
        status: SideTurnStatus::Running,
        created_at: 3,
        updated_at: 4,
        error: None,
    });
    store.update_topic("agent", followup).unwrap();
    store.update_topic("agent", first).unwrap();
    let (retained, retained_snapshot) = store.topic("agent", "topic").unwrap().unwrap();
    assert_eq!(retained.latest_turn().question, "followup");
    assert_eq!(retained.latest_turn().status, SideTurnStatus::Running);
    assert_eq!(retained_snapshot, frozen);
}

#[test]
fn projection_bound_never_discards_durable_followups_or_frozen_context() {
    let store = SideStore::memory().expect("side store");
    let mut history = topic();
    for index in 0..12 {
        history.record.follow_ups.push(BtwHistoryTurn {
            question: format!("question {index}"),
            images: Vec::new(),
            answer: format!("answer {index}"),
            status: SideTurnStatus::Complete,
            created_at: index + 10,
            updated_at: index + 11,
            error: None,
        });
    }
    let frozen = snapshot();
    store
        .insert_topic("agent", history.clone(), &frozen)
        .expect("store full history");
    let projected = store.topics("agent").expect("bounded projection");
    assert_eq!(projected[0].record.turn_count(), side_questions::MAX_TURNS);
    assert!(projected[0].truncated);
    assert_eq!(projected[0].latest_turn().answer, "answer 11");
    let (full, context) = store
        .topic("agent", "topic")
        .expect("topic read")
        .expect("topic");
    assert_eq!(full, history);
    assert_eq!(full.record.turn_count(), 13);
    assert_eq!(context, frozen);
}

#[test]
fn side_context_redaction_preserves_source_urls_but_scrubs_credentials() {
    let text =
        "See https://user:pass@example.test/docs/guide?token=hidden and api_key=fixture-secret";
    let scrubbed = redact_text(text);
    assert!(scrubbed.contains("https://example.test/docs/guide"));
    assert!(!scrubbed.contains("user:pass"));
    assert!(!scrubbed.contains("hidden"));
    assert!(!scrubbed.contains("fixture-secret"));
}

#[test]
fn deletion_removes_only_the_source_agents_history_receipts_and_owned_auxiliary_directory() {
    let directory = tempfile::tempdir().expect("side fixture");
    let store = SideStore::open(&directory.path().join("side.sqlite3")).expect("side store");
    store
        .insert_topic("agent", topic(), &snapshot())
        .expect("source history");
    store
        .insert_topic("other", topic(), &snapshot())
        .expect("unrelated history");
    store
        .claim_ask("agent", "request", "question", None)
        .expect("source receipt");
    let auxiliary = store.directory("agent").expect("owned auxiliary path");
    std::fs::create_dir_all(&auxiliary).expect("auxiliary fixture");
    std::fs::write(auxiliary.join("fixture"), "fixture").expect("owned fixture");
    store.forget("agent").expect("delete auxiliary state");
    assert!(
        store
            .topic("agent", "topic")
            .expect("source read")
            .is_none()
    );
    assert!(
        store
            .ask("agent", "request")
            .expect("receipt read")
            .is_none()
    );
    assert!(
        store
            .topic("other", "topic")
            .expect("unrelated read")
            .is_some()
    );
    assert!(!auxiliary.exists());
}
