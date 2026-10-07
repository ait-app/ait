use model::{
    ErrorCode,
    creation::protocol::Kind,
    creation::{Creations, validate_key},
    outbound::Outbound,
};
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
        .advance(&admitted.snapshot, "failed", None, Some("failed".into()))
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
    let ready = creations.advance(&admission.snapshot, "agent_ready",
        Some(json!({"agent":{"id":admission.snapshot.agent_id,"workspaceId":"wks_0123456789abcdef"}})), None).unwrap();
    assert_eq!(ready.workspace_id.as_deref(), Some("wks_0123456789abcdef"));
    let failed = creations
        .advance(&ready, "failed", None, Some("uncertain prompt".into()))
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
            "failed",
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
    assert_eq!(resumed.snapshot.phase, "workspace_ready");
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
            "completed",
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
            .advance(&completed, "completed", None, None)
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
            "failed",
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
