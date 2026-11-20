use domain::creation::protocol::{Kind, Phase};
use model::ErrorCode;
use model::creation::{Creations, validate_key};
use model::outbound::Outbound;
use serde_json::{Value, json};

use super::*;

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
    let creations = open(&path).unwrap();
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
    let creations = open(&path).unwrap();
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
    let creations = open(&path).unwrap();
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
    let creations = open(&path).unwrap();
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
    let creations = open(&path).unwrap();
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
    let creations = open(&path).unwrap();
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
    let creations = open(&path).unwrap();
    let admitted = creations.begin(Kind::Workspace, "key", json!({})).unwrap();
    assert!(
        !creations
            .snapshot(Kind::Workspace, "key")
            .unwrap()
            .unwrap()
            .outcome_unknown
    );
    drop(creations);
    let creations = open(&path).unwrap();
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
    fs::write(&bad, "not json").unwrap();
    assert!(open(&bad).is_err());
}

#[test]
fn imports_large_legacy_receipts_and_replays_them_after_new_writes() {
    let root = tempfile::tempdir().unwrap();
    let legacy = root.path().join("receipts.json");
    let intent = json!({"agent": {"images": ["A".repeat(33 * 1024 * 1024)]}});
    let receipt = legacy_receipt(intent.clone());
    let expected = receipt.snapshot.clone();
    let mut writer = io::BufWriter::new(fs::File::create(&legacy).unwrap());
    serde_json::to_writer(&mut writer, &[receipt]).unwrap();
    writer.into_inner().unwrap();
    assert!(legacy.metadata().unwrap().len() > crate::registry::MAX_REGISTRY_BYTES);
    let original = fs::read(&legacy).unwrap();

    let creations = open(&legacy).unwrap();
    let replay = creations.begin(Kind::Workspace, "legacy", intent).unwrap();
    assert!(!replay.execute);
    assert_eq!(replay.snapshot, expected);
    let new = creations.begin(Kind::Agent, "new", json!({})).unwrap();
    assert!(new.execute);
    drop(creations);

    assert_eq!(fs::read(&legacy).unwrap(), original);
    let database = legacy.with_extension("sqlite3");
    assert!(database.metadata().unwrap().len() > crate::registry::MAX_REGISTRY_BYTES);
    // The completed import must never replay a stale backup over newer database records.
    fs::write(&legacy, b"no longer valid JSON").unwrap();
    let reopened = open(&legacy).unwrap();
    assert_eq!(
        reopened.snapshot(Kind::Workspace, "legacy").unwrap(),
        Some(expected)
    );
    assert_eq!(
        reopened
            .snapshot(Kind::Agent, "new")
            .unwrap()
            .unwrap()
            .agent_id,
        new.snapshot.agent_id
    );
}

#[test]
fn malformed_legacy_input_rolls_back_the_import_and_can_be_retried() {
    let root = tempfile::tempdir().unwrap();
    let legacy = root.path().join("receipts.json");
    let receipt = legacy_receipt(json!({}));
    let malformed = format!("[{},", serde_json::to_string(&receipt).unwrap());
    fs::write(&legacy, &malformed).unwrap();

    assert!(matches!(open(&legacy), Err(ErrorCode::RegistryIo)));
    assert_eq!(fs::read_to_string(&legacy).unwrap(), malformed);
    let connection = Connection::open(legacy.with_extension("sqlite3")).unwrap();
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 0);
    assert!(!connection.table_exists(None, "receipts").unwrap());
    drop(connection);

    fs::write(&legacy, serde_json::to_vec(&[&receipt]).unwrap()).unwrap();
    let reopened = open(&legacy).unwrap();
    assert_eq!(
        reopened.snapshot(Kind::Workspace, "legacy").unwrap(),
        Some(receipt.snapshot)
    );
}

fn legacy_receipt(intent: Value) -> Receipt {
    use sha2::{Digest, Sha256};

    Receipt {
        id: format!("workspace:{:x}", Sha256::digest(b"legacy")),
        intent,
        snapshot: Snapshot {
            kind: Kind::Workspace,
            idempotency_key: "legacy".to_owned(),
            revision: 1,
            phase: Phase::Completed,
            workspace_id: Some("wks_0123456789abcdef".to_owned()),
            agent_id: None,
            error: None,
            outcome_unknown: false,
            workspace: None,
            agent: None,
        },
        retry_initial_agent: false,
    }
}

#[test]
fn sqlite_disk_limit_preserves_committed_receipts() {
    let root = tempfile::tempdir().unwrap();
    let legacy = root.path().join("receipts.json");
    let mut connection = open_database(&legacy.with_extension("sqlite3")).unwrap();
    migration::initialize(&mut connection, &legacy).unwrap();
    let store = SqliteReceiptStore {
        connection: Mutex::new(connection),
    };
    let saved = legacy_receipt(json!({}));
    store.put(saved.clone()).unwrap();
    {
        let connection = store.lock().unwrap();
        let pages: u64 = connection
            .pragma_query_value(None, "page_count", |row| row.get(0))
            .unwrap();
        connection
            .pragma_update(None, "max_page_count", pages)
            .unwrap();
    }

    let too_large = legacy_receipt(json!({"images": ["A".repeat(64 * 1024)]}));
    assert_eq!(store.put(too_large), Err(ErrorCode::ResourceExhausted));
    let committed = store.get(&saved.id).unwrap().unwrap();
    assert_eq!(committed.intent, saved.intent);
    assert_eq!(committed.snapshot, saved.snapshot);
}
