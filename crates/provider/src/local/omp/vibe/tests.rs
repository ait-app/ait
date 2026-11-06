use super::*;
use serde_json::json;

fn greeting(version: u64, sdk: Value, vibe: Value) -> Value {
    json!({
        "type": "ait_native_host_hello",
        "bridgeVersion": version,
        "sdkVersion": sdk,
        "capabilities": {"vibe": vibe},
    })
}

const ROOT: &str = "11111111-1111-4111-8111-111111111111";
const CHILD: &str = "22222222-2222-4222-8222-222222222222";
const GRANDCHILD: &str = "33333333-3333-4333-8333-333333333333";

fn state_frame(enabled: bool) -> Value {
    json!({"type":"ait_vibe_state",
        "state":{"enabled":enabled,"entering":false,"exiting":false},
        "runtime":{"bridgeVersion":1,"sdkVersion":"18.17.0"}})
}

fn worker_frame(id: &str, parent: &str, status: &str) -> Value {
    json!({"type":"ait_vibe_worker","worker":{
        "id":id,"workerId":"worker-alias","parentId":parent,
        "sessionFile":"/tmp/x/worker.jsonl","parentSessionFile":"/tmp/x/root.jsonl",
        "cwd":"/tmp/x","ownerId":"main","createdAt":"2026-01-01T00:00:00.000Z",
        "updatedAt":"2026-01-01T00:00:05.000Z","status":status,"outcomeKnown":true,
        "nativeState":"running","description":"Inspect the parser","model":"gpt-5",
        "directWorker":true}})
}

#[test]
fn decodes_supported_greeting() {
    assert_eq!(
        hello(&greeting(1, json!("0.4.2"), json!(true))),
        Some(Capability {
            bridge_version: 1,
            sdk_version: "0.4.2".to_owned(),
            vibe: true,
        })
    );
}

#[test]
fn disables_vibe_on_unknown_bridge_version() {
    let capability = hello(&greeting(2, json!("0.4.2"), json!(true))).unwrap();
    assert_eq!(capability.bridge_version, 2);
    assert!(!capability.vibe);
}

#[test]
fn keeps_handshake_when_vibe_is_unavailable() {
    let capability = hello(&greeting(1, json!("0.5.0"), json!(false))).unwrap();
    assert_eq!(capability.sdk_version, "0.5.0");
    assert!(!capability.vibe);
}

#[test]
fn rejects_foreign_and_malformed_frames() {
    assert!(hello(&json!({"type": "ready"})).is_none());
    assert!(hello(&json!({"type": "ait_native_host_hello"})).is_none());
    assert!(hello(&greeting(1, json!(4), json!(true))).is_none());
    assert!(hello(&greeting(1, json!("0.4.2"), json!("yes"))).is_none());
    assert!(hello(&greeting(1, json!("0.4.2"), json!(null))).is_none());
    assert!(
        hello(&json!({
            "type": "ait_native_host_hello",
            "bridgeVersion": -1,
            "sdkVersion": "0.4.2",
            "capabilities": {"vibe": true},
        }))
        .is_none()
    );
    assert!(
        hello(&json!({
            "type": "ait_native_host_hello",
            "bridgeVersion": 4_294_967_296_u64,
            "sdkVersion": "0.4.2",
            "capabilities": {"vibe": true},
        }))
        .is_none()
    );
}

#[test]
fn only_adapter_frames_are_intercepted() {
    assert!(is_frame(&state_frame(true)));
    assert!(is_frame(&worker_frame(CHILD, ROOT, "running")));
    for frame in [
        json!({"type":"ready"}),
        json!({"type":"message_end"}),
        json!({"type":"subagent_lifecycle"}),
    ] {
        assert!(!is_frame(&frame));
    }
}

#[test]
fn state_frame_projects_enabled_and_drops_older_state() {
    let mut state = State::default();
    assert!(state.capture(&state_frame(true), ROOT, "/tmp/x").is_none());
    assert!(state.enabled);
    assert!(state.capture(&state_frame(false), ROOT, "/tmp/x").is_none());
    assert!(!state.enabled);
    assert_eq!(state.snapshot(), json!({"enabled":false,"workers":[]}));
}

#[test]
fn worker_frame_becomes_a_child_upsert_with_verified_parent() {
    let mut state = State::default();
    let event = state
        .capture(&worker_frame(CHILD, ROOT, "running"), ROOT, "/tmp/x")
        .unwrap();
    let AgentTurnEvent::Subagent(SubagentEvent::Upsert(child)) = event else {
        panic!("a worker frame must publish a child upsert");
    };
    assert_eq!(child.id, CHILD);
    assert_eq!(child.parent_id, ROOT);
    assert_eq!(child.cwd, "/tmp/x");
    assert!(child.persistence.is_none());
    assert_eq!(child.descriptor["status"], "running");
    assert_eq!(child.descriptor["description"], "Inspect the parser");
    assert_eq!(child.descriptor["provider"], super::super::PROVIDER);
    assert_eq!(
        state.snapshot()["workers"],
        json!([{"id":CHILD,"parentId":ROOT,"status":"running",
            "description":"Inspect the parser","model":"gpt-5"}])
    );
}

#[test]
fn worker_upsert_replaces_in_place_and_keeps_parent_chain() {
    let mut state = State::default();
    state.capture(&worker_frame(CHILD, ROOT, "running"), ROOT, "/tmp/x");
    state.capture(&worker_frame(CHILD, ROOT, "completed"), ROOT, "/tmp/x");
    assert_eq!(state.workers.len(), 1);
    assert_eq!(state.workers[CHILD].descriptor["status"], "completed");
    // A nested worker may name an already verified worker as its parent.
    state.capture(&worker_frame(GRANDCHILD, CHILD, "running"), ROOT, "/tmp/x");
    assert_eq!(state.workers[GRANDCHILD].parent_id, CHILD);
    assert_eq!(state.workers.len(), 2);
}

#[test]
fn unverified_worker_identities_are_dropped() {
    let mut state = State::default();
    // Unknown parent: a native UUID that no verified header established.
    assert!(
        state
            .capture(&worker_frame(CHILD, GRANDCHILD, "running"), ROOT, "/tmp/x")
            .is_none()
    );
    // Not a canonical UUID.
    assert!(
        state
            .capture(
                &worker_frame("worker-alias", ROOT, "running"),
                ROOT,
                "/tmp/x"
            )
            .is_none()
    );
    // Self-parenting is not the root.
    assert!(
        state
            .capture(&worker_frame(CHILD, CHILD, "running"), ROOT, "/tmp/x")
            .is_none()
    );
    assert!(state.workers.is_empty());
}

#[test]
fn outbox_keeps_one_state_frame_and_upserts_workers_by_id() {
    let mut frames = Vec::new();
    assert!(store(&mut frames, &state_frame(true)));
    assert!(store(&mut frames, &state_frame(false)));
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0]["state"]["enabled"], false);
    assert!(store(&mut frames, &worker_frame(CHILD, ROOT, "running")));
    assert!(store(&mut frames, &worker_frame(CHILD, ROOT, "completed")));
    let workers: Vec<_> = frames
        .iter()
        .filter(|frame| frame["type"] == "ait_vibe_worker")
        .collect();
    assert_eq!(workers.len(), 1);
    assert_eq!(workers[0]["worker"]["status"], "completed");
    // Foreign, unnamed and unnamed-worker frames are never retained.
    assert!(!store(&mut frames, &json!({"type":"ready"})));
    assert!(!store(
        &mut frames,
        &json!({"type":"ait_vibe_worker","worker":{}})
    ));
    assert_eq!(frames.len(), 2);
}

#[test]
fn outbox_is_bounded_to_the_native_worker_limit() {
    let mut frames = Vec::new();
    for index in 0..MAX_WORKERS + 8 {
        let id = format!("00000000-0000-4000-8000-{index:012}");
        let retained = store(&mut frames, &worker_frame(&id, ROOT, "running"));
        // The outbox admits exactly the native descendant bound and drops the overflow.
        assert_eq!(retained, index < MAX_WORKERS);
    }
    assert_eq!(
        frames
            .iter()
            .filter(|frame| frame["type"] == "ait_vibe_worker")
            .count(),
        MAX_WORKERS
    );
    let first = format!("00000000-0000-4000-8000-{:012}", 0);
    assert!(store(&mut frames, &worker_frame(&first, ROOT, "completed")));
    assert_eq!(
        frames
            .iter()
            .filter(|frame| frame["type"] == "ait_vibe_worker")
            .count(),
        MAX_WORKERS
    );
}

#[test]
fn state_carries_no_workers_until_a_worker_frame_arrives() {
    let mut state = State::default();
    state.capture(&state_frame(true), ROOT, "/tmp/x");
    assert_eq!(state.snapshot(), json!({"enabled":true,"workers":[]}));
    assert_eq!(state.workers.len(), 0);
}

#[test]
fn snapshot_clips_oversized_native_text() {
    let huge = "x".repeat(9000);
    let mut frame = worker_frame(CHILD, ROOT, "running");
    frame["worker"]["description"] = json!(huge);
    let mut state = State::default();
    state.capture(&frame, ROOT, "/tmp/x");
    let description = state.snapshot()["workers"][0]["description"]
        .as_str()
        .unwrap()
        .len();
    assert_eq!(description, 4096);
}
