use std::sync::atomic::Ordering;

use super::*;
use crate::host::hub::tests::{MemoryStore, scope};

fn event() -> Value {
    json!({"eventId":Uuid::new_v4(),"method":"agent_attention_required",
        "params":{"agentId":"agent","timestamp":"now","reason":"finished"}})
}

#[test]
fn restart_retry_and_duplicate_attention_preserve_one_identity() {
    let store = Arc::new(MemoryStore::default());
    let scope = scope();
    let mut outbox = Outbox::open(store.clone()).unwrap();
    outbox.activate(&scope, 0).unwrap();
    let value = event();
    outbox.enqueue(&scope, value.clone(), 1).unwrap();
    outbox.enqueue(&scope, event(), 2).unwrap();
    let mut recovered = Outbox::open(store.clone()).unwrap();
    let job = recovered.next(&scope, 3).unwrap();
    assert_eq!(job.entry.params, value);
    recovered.complete(&job, None).unwrap();
    recovered.enqueue(&scope, event(), 4).unwrap();
    assert!(recovered.next(&scope, 4).is_none());
    assert!(Outbox::open(store).unwrap().next(&scope, 4).is_none());
}

#[test]
fn failed_commits_leave_memory_and_disk_retryable() {
    let store = Arc::new(MemoryStore::default());
    let scope = scope();
    let mut outbox = Outbox::open(store.clone()).unwrap();
    outbox.activate(&scope, 0).unwrap();
    store.fail.store(true, Ordering::Relaxed);
    assert!(outbox.enqueue(&scope, event(), 1).is_err());
    assert!(outbox.next(&scope, 1).is_none());
    store.fail.store(false, Ordering::Relaxed);
    outbox.enqueue(&scope, event(), 2).unwrap();
    let job = outbox.next(&scope, 2).unwrap();
    store.fail.store(true, Ordering::Relaxed);
    assert!(outbox.complete(&job, None).is_err());
    assert_eq!(outbox.next(&scope, 2).unwrap().entry.id, job.entry.id);
    assert_eq!(
        Outbox::open(store)
            .unwrap()
            .next(&scope, 2)
            .unwrap()
            .entry
            .id,
        job.entry.id
    );
}

#[test]
fn authority_switch_does_not_replay_previous_account_events_or_revisions() {
    let mut outbox = Outbox::open(Arc::new(MemoryStore::default())).unwrap();
    let first = scope();
    outbox.activate(&first, 0).unwrap();
    outbox.enqueue(&first, event(), 1).unwrap();
    let old = outbox.next(&first, 1).unwrap();
    outbox.snapshot("workspace", json!([]), false, 1).unwrap();
    let second = scope();
    assert!(outbox.next(&second, 2).is_none());
    outbox.activate(&second, 2).unwrap();
    assert!(outbox.enqueue(&first, event(), 3).is_err());
    outbox.snapshot("workspace", json!([]), false, 3).unwrap();
    let current = outbox.next(&second, 3).unwrap();
    assert_eq!(current.entry.params["revision"], 1);
    outbox.complete(&old, None).unwrap();
    assert_eq!(outbox.next(&second, 4).unwrap().entry.id, current.entry.id);
}

#[test]
fn snapshots_coalesce_and_late_ack_cannot_remove_a_newer_revision() {
    let mut outbox = Outbox::open(Arc::new(MemoryStore::default())).unwrap();
    let scope = scope();
    outbox.activate(&scope, 0).unwrap();
    outbox
        .snapshot(
            "agent",
            json!([{"id":"agent","data":{"title":"First"}}]),
            false,
            0,
        )
        .unwrap();
    let first = outbox.next(&scope, 0).unwrap();
    outbox.snapshot("agent", json!([]), false, 1).unwrap();
    let second = outbox.next(&scope, 1).unwrap();
    assert_eq!(second.entry.params["revision"], 2);
    outbox.complete(&first, None).unwrap();
    assert_eq!(outbox.next(&scope, 1).unwrap().entry.id, second.entry.id);
    outbox.complete(&second, None).unwrap();
    outbox.snapshot("agent", json!([]), false, 2).unwrap();
    assert!(outbox.next(&scope, 2).is_none());
    outbox.snapshot("agent", json!([]), true, 3).unwrap();
    assert_eq!(
        outbox.next(&scope, 3).unwrap().entry.params,
        second.entry.params
    );
}

#[test]
fn quarantine_and_expiration_do_not_block_other_events() {
    let mut outbox = Outbox::open(Arc::new(MemoryStore::default())).unwrap();
    let scope = scope();
    outbox.activate(&scope, 0).unwrap();
    outbox.enqueue(&scope, event(), 0).unwrap();
    let first = outbox.next(&scope, 1).unwrap();
    outbox
        .complete(&first, Some("conflict".to_owned()))
        .unwrap();
    assert!(outbox.next(&scope, 1).is_none());
    outbox.activate(&scope, RETENTION_MS).unwrap();
    outbox.enqueue(&scope, event(), RETENTION_MS).unwrap();
    assert!(outbox.next(&scope, RETENTION_MS).is_some());
    assert!(outbox.next(&scope, RETENTION_MS * 2).is_none());
}

#[test]
fn invalid_journal_versions_and_oversized_snapshots_are_rejected() {
    let store = Arc::new(MemoryStore::default());
    *store.document.lock().unwrap() =
        json!({"version":2,"scope":null,"events":[],"directories":{},"seen":{}});
    assert!(Outbox::open(store).is_err());
    let mut outbox = Outbox::open(Arc::new(MemoryStore::default())).unwrap();
    let scope = scope();
    outbox.activate(&scope, 0).unwrap();
    assert!(
        outbox
            .snapshot("project", json!(["x".repeat(1024 * 1024)]), false, 0)
            .is_err()
    );
    assert!(outbox.next(&scope, 0).is_none());
}
