use domain::creation::protocol::{Kind, Phase};
use model::ErrorCode;
use model::creation::{Creations, validate_key};
use model::outbound::Outbound;
use serde_json::{Value, json};

use super::*;

fn legacy(snapshot: &Snapshot, intent: &Value) -> Value {
    use sha2::{Digest, Sha256};
    let id = format!(
        "{}:{:x}",
        snapshot.kind.name(),
        Sha256::digest(snapshot.idempotency_key.as_bytes())
    );
    json!({"id":id, "snapshot":snapshot, "intent":intent})
}

#[test]
fn oversized_legacy_receipts_restore_claims_and_allow_durable_progress() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("receipts.json");
    let creations = Creations::default();
    let admitted = creations.begin(Kind::Agent, "large", json!({})).unwrap();
    let completed = creations
        .advance(&admitted.snapshot, Phase::Completed, None, None)
        .unwrap();
    let pending = creations
        .begin(Kind::Workspace, "pending", json!({}))
        .unwrap();
    drop(creations);

    // Older releases could persist more than the loader's 32 MiB aggregate limit.
    let intent = json!({"images": ["x".repeat(43 * 1024 * 1024)]});
    let rows = json!([
        legacy(&completed, &intent),
        legacy(&pending.snapshot, &json!({}))
    ]);
    serde_json::to_writer(std::fs::File::create(&path).unwrap(), &rows).unwrap();
    drop(rows);
    assert!(std::fs::metadata(&path).unwrap().len() > crate::registry::MAX_REGISTRY_BYTES);

    let creations = open(path.clone()).unwrap();
    assert!(std::fs::metadata(&path).unwrap().len() < 64 * 1024);
    let replay = creations.begin(Kind::Agent, "large", intent).unwrap();
    assert!(!replay.execute);
    assert_eq!(replay.snapshot, completed);
    assert!(matches!(
        creations.begin(Kind::Agent, "large", json!({})),
        Err(ErrorCode::IdempotencyConflict)
    ));
    assert!(matches!(
        creations.begin(
            Kind::Agent,
            "duplicate-resource",
            json!({"agentId": completed.agent_id})
        ),
        Err(ErrorCode::IdempotencyConflict)
    ));
    let interrupted = creations
        .snapshot(Kind::Workspace, "pending")
        .unwrap()
        .unwrap();
    assert!(interrupted.outcome_unknown);
    assert_eq!(interrupted.workspace_id, pending.snapshot.workspace_id);

    let created = creations.begin(Kind::Agent, "new", json!({})).unwrap();
    assert!(created.execute);
    let progressed = creations
        .advance(&created.snapshot, Phase::Completed, None, None)
        .unwrap();
    drop(creations);
    let reopened = open(path).unwrap();
    assert_eq!(
        reopened.snapshot(Kind::Agent, "large").unwrap(),
        Some(completed)
    );
    assert_eq!(
        reopened.snapshot(Kind::Agent, "new").unwrap(),
        Some(progressed)
    );
    assert_eq!(
        reopened.snapshot(Kind::Workspace, "pending").unwrap(),
        Some(interrupted)
    );
}

#[test]
fn malformed_or_unreadable_receipts_fail_without_changing_the_file() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("receipts.json");
    for contents in ["[", "[] trailing", r#"[{"id":"private-prompt"}]"#] {
        std::fs::write(&path, contents).unwrap();
        assert!(open(path.clone()).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), contents);
    }
    assert!(open(root.path().to_path_buf()).is_err());
}

#[test]
fn opening_and_replaying_a_key_do_not_decode_other_receipts() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("receipts.json");
    let creations = open(path.clone()).unwrap();
    let good = creations.begin(Kind::Agent, "good", json!({})).unwrap();
    let result = creations
        .advance(&good.snapshot, Phase::Completed, None, None)
        .unwrap();
    let bad = creations.begin(Kind::Agent, "bad", json!({})).unwrap();
    drop(creations);
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE receipts SET snapshot=zeroblob(?1) WHERE agent_claim=?2",
        params![
            crate::registry::MAX_REGISTRY_BYTES + 1,
            bad.snapshot.agent_id
        ],
    )
    .unwrap();
    drop(db);
    assert!(std::fs::metadata(&path).unwrap().len() > crate::registry::MAX_REGISTRY_BYTES);

    let reopened = open(path.clone()).unwrap();
    assert_eq!(
        reopened
            .begin(Kind::Agent, "good", json!({}))
            .unwrap()
            .snapshot,
        result
    );
    assert_eq!(
        reopened.snapshot(Kind::Agent, "bad"),
        Err(ErrorCode::RegistryIo)
    );
    assert!(
        reopened
            .begin(Kind::Agent, "new", json!({}))
            .unwrap()
            .execute
    );
    drop(reopened);
    let db = Connection::open(&path).unwrap();
    db.execute("DROP TABLE receipts", []).unwrap();
    drop(db);
    assert!(open(path).is_err());
}

#[test]
fn operation_progress_is_untagged_and_ends_with_its_guard() {
    let creations = Creations::default();
    let (outbound, mut receiver) = Outbound::new();
    assert!(
        creations
            .observe(Kind::Agent, "", outbound.clone())
            .is_err()
    );
    let operation = creations
        .observe(Kind::Agent, "operation", outbound)
        .unwrap();
    operation.activate().unwrap();
    let admitted = creations
        .begin(Kind::Agent, "operation", json!({}))
        .unwrap();
    let frame = receiver.try_recv().unwrap();
    let model::outbound::Frame::Text(text) = frame.message else {
        panic!("Expected a creation event");
    };
    let event: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(event["params"]["phase"], "accepted");
    assert!(event["params"].get("subscriptionId").is_none());
    drop(operation);
    creations
        .advance(
            &admitted.snapshot,
            Phase::Failed,
            None,
            Some("failed".into()),
        )
        .unwrap();
    assert!(receiver.try_recv().is_err());
}

#[test]
fn different_intents_cannot_claim_the_same_resources_even_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("receipts.json");
    let creations = open(path.clone()).unwrap();
    let workspace = json!({"workspaceId":"wks_0123456789abcdef",
        "agent":{"agentId":"01234567-89ab-4def-8123-0123456789ab"}});
    creations
        .begin(Kind::Workspace, "one", workspace.clone())
        .unwrap();
    assert!(matches!(
        creations.begin(Kind::Workspace, "two", workspace.clone()),
        Err(ErrorCode::IdempotencyConflict)
    ));
    drop(creations);
    let creations = open(path).unwrap();
    assert!(matches!(
        creations.begin(
            Kind::Agent,
            "three",
            json!({"agentId":workspace["agent"]["agentId"]})
        ),
        Err(ErrorCode::IdempotencyConflict)
    ));
    // An Agent borrows an existing Workspace instead of claiming it for creation.
    for key in ["four", "five"] {
        assert!(
            creations
                .begin(
                    Kind::Agent,
                    key,
                    json!({"workspaceId":workspace["workspaceId"]})
                )
                .unwrap()
                .execute
        );
    }
}

#[test]
fn committed_agent_records_placement_and_uncertain_prompt_failure() {
    let creations = Creations::default();
    let admission = creations.begin(Kind::Agent, "prompt", json!({})).unwrap();
    let ready = creations.advance(&admission.snapshot, Phase::AgentReady,
        Some(json!({"agent":{"id":admission.snapshot.agent_id,"workspaceId":"wks_0123456789abcdef"}})), None).unwrap();
    assert_eq!(ready.workspace_id.as_deref(), Some("wks_0123456789abcdef"));
    let failed = creations
        .advance(&ready, Phase::Failed, None, Some("uncertain prompt".into()))
        .unwrap();
    assert!(failed.outcome_unknown);
    assert_eq!(
        creations
            .begin(Kind::Agent, "prompt", json!({}))
            .unwrap()
            .snapshot,
        failed
    );
}

#[test]
fn proven_initial_agent_startup_failure_retries_reserved_ids_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("receipts.json");
    let creations = open(path.clone()).unwrap();
    let intent = json!({"agent":{"config":{"provider":"codex"}}});
    let admission = creations
        .begin(Kind::Workspace, "retry", intent.clone())
        .unwrap();
    let failed = creations
        .advance(
            &admission.snapshot,
            Phase::Failed,
            Some(json!({"workspace":{"id":admission.snapshot.workspace_id}})),
            Some("startup".into()),
        )
        .unwrap();
    assert!(
        !creations
            .begin(Kind::Workspace, "retry", intent.clone())
            .unwrap()
            .execute
    );
    creations.allow_initial_agent_retry(&failed).unwrap();
    drop(creations);
    let creations = open(path).unwrap();
    let resumed = creations
        .begin(Kind::Workspace, "retry", intent.clone())
        .unwrap();
    assert!(resumed.execute);
    assert_eq!(resumed.snapshot.workspace_id, failed.workspace_id);
    assert_eq!(resumed.snapshot.agent_id, failed.agent_id);
    assert_eq!(resumed.snapshot.phase, Phase::WorkspaceReady);
    assert!(resumed.snapshot.error.is_none());
    assert!(
        !creations
            .begin(Kind::Workspace, "retry", intent)
            .unwrap()
            .execute
    );
    assert!(creations.allow_initial_agent_retry(&failed).is_err());
}

#[test]
fn creation_replays_committed_results_without_repeating_effects_and_rejects_conflicts() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("receipts.json");
    let creations = open(path.clone()).unwrap();
    let intent = json!({"config":{"provider":"codex"}});
    let admitted = creations.begin(Kind::Agent, "key", intent.clone()).unwrap();
    assert!(admitted.execute);
    assert!(admitted.snapshot.agent_id.is_some());
    let completed = creations
        .advance(
            &admitted.snapshot,
            Phase::Completed,
            Some(json!({"agent":{"id":admitted.snapshot.agent_id}})),
            None,
        )
        .unwrap();
    assert_eq!(completed.revision, 1);
    assert!(matches!(
        creations.begin(Kind::Agent, "key", json!({"changed":true})),
        Err(ErrorCode::IdempotencyConflict)
    ));
    drop(creations);
    let creations = open(path).unwrap();
    let replay = creations.begin(Kind::Agent, "key", intent).unwrap();
    assert!(!replay.execute);
    assert_eq!(replay.snapshot, completed);
    assert!(
        creations
            .advance(&completed, Phase::Completed, None, None)
            .is_err()
    );
    assert!(
        creations
            .snapshot(Kind::Workspace, "key")
            .unwrap()
            .is_none()
    );
}

#[test]
fn interrupted_creation_is_observable_without_launching_a_second_attempt() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("receipts.json");
    let creations = open(path.clone()).unwrap();
    let admitted = creations.begin(Kind::Workspace, "key", json!({})).unwrap();
    assert!(
        !creations
            .snapshot(Kind::Workspace, "key")
            .unwrap()
            .unwrap()
            .outcome_unknown
    );
    drop(creations);
    let creations = open(path).unwrap();
    let snapshot = creations.snapshot(Kind::Workspace, "key").unwrap().unwrap();
    assert!(snapshot.outcome_unknown);
    assert_eq!(snapshot.workspace_id, admitted.snapshot.workspace_id);
    assert!(
        !creations
            .begin(Kind::Workspace, "key", json!({}))
            .unwrap()
            .execute
    );
}

#[test]
fn observer_can_arrive_before_creation_and_releases_without_affecting_work() {
    let creations = Creations::default();
    let (outbound, mut receiver) = Outbound::new();
    let (snapshot, subscription) = creations.subscribe(Kind::Agent, "key", outbound).unwrap();
    assert!(snapshot.is_none());
    let admitted = creations.begin(Kind::Agent, "key", json!({})).unwrap();
    assert!(receiver.try_recv().is_err());
    subscription.activate().unwrap();
    assert!(receiver.try_recv().is_ok());
    drop(subscription);
    creations
        .advance(
            &admitted.snapshot,
            Phase::Failed,
            None,
            Some("failed".to_owned()),
        )
        .unwrap();
    assert!(receiver.try_recv().is_err());
    assert!(validate_key("").is_err());
    assert!(validate_key(&"x".repeat(513)).is_err());
    assert!(creations.begin(Kind::Agent, "\n", json!({})).is_err());
    let root = tempfile::tempdir().unwrap();
    let bad = root.path().join("bad");
    std::fs::write(&bad, "not json").unwrap();
    assert!(open(bad).is_err());
}

#[test]
fn migration_can_retry_after_partial_input_or_abandoned_staging() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("receipts.json");
    let snapshot = Creations::default()
        .begin(Kind::Agent, "one", json!({}))
        .unwrap()
        .snapshot;
    let row = legacy(&snapshot, &json!({}));
    let partial = format!("[{row},");
    std::fs::write(&path, &partial).unwrap();
    std::fs::write(path.with_added_extension("migrating"), b"aborted import").unwrap();
    assert!(open(path.clone()).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), partial);
    assert!(!path.with_added_extension("migrating").exists());

    let mut last = row.clone();
    last["snapshot"]["phase"] = json!("completed");
    last["snapshot"]["revision"] = json!(1);
    let other = Creations::default()
        .begin(Kind::Agent, "two", json!({"agentId":snapshot.agent_id}))
        .unwrap()
        .snapshot;
    let rows = json!([
        row,
        last,
        legacy(&other, &json!({"agentId":snapshot.agent_id}))
    ]);
    serde_json::to_writer(std::fs::File::create(&path).unwrap(), &rows).unwrap();
    let creations = open(path.clone()).unwrap();
    assert_eq!(
        creations
            .snapshot(Kind::Agent, "one")
            .unwrap()
            .unwrap()
            .phase,
        Phase::Completed
    );
    assert!(creations.snapshot(Kind::Agent, "two").unwrap().is_some());
    assert!(matches!(
        creations.begin(Kind::Agent, "three", json!({"agentId":snapshot.agent_id})),
        Err(ErrorCode::IdempotencyConflict)
    ));
}

#[test]
fn sqlite_full_preserves_committed_progress_and_other_keys_can_still_replay() {
    use sha2::{Digest, Sha256};

    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("receipts.json");
    let store = Arc::new(Store(Mutex::new(migration::open(&path).unwrap())));
    let creations = Creations::with_store(store.clone());
    let old = creations.begin(Kind::Agent, "old", json!({})).unwrap();
    let completed = creations
        .advance(&old.snapshot, Phase::Completed, None, None)
        .unwrap();
    let admission = creations
        .begin(Kind::Workspace, "pending", json!({}))
        .unwrap();
    {
        let db = store.0.lock().unwrap();
        let pages: u32 = db
            .pragma_query_value(None, "page_count", |r| r.get(0))
            .unwrap();
        db.pragma_update(None, "max_page_count", pages).unwrap();
    }
    let payload: Vec<_> = (0u32..4096)
        .map(|i| format!("{:x}", Sha256::digest(i.to_be_bytes())))
        .collect();
    let result = creations.advance(
        &admission.snapshot,
        Phase::Completed,
        Some(json!({"workspace":{"content":payload}})),
        None,
    );
    assert_eq!(result, Err(ErrorCode::ResourceExhausted));
    assert_eq!(
        creations.snapshot(Kind::Workspace, "pending").unwrap(),
        Some(admission.snapshot.clone())
    );
    assert_eq!(
        creations
            .begin(Kind::Agent, "old", json!({}))
            .unwrap()
            .snapshot,
        completed
    );
    drop(creations);
    drop(store);
    let reopened = open(path).unwrap();
    let retry = reopened
        .begin(Kind::Workspace, "pending", json!({}))
        .unwrap();
    assert!(!retry.execute);
    assert!(retry.snapshot.outcome_unknown);
}
