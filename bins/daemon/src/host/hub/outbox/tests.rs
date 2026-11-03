use std::sync::atomic::Ordering;

use super::*;
use crate::host::hub::tests::{MemoryStore, methods, scope};

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
    let job = recovered.next(&scope, &methods(), 3).unwrap();
    assert_eq!(job.entry.params, value);
    recovered.complete(&job, None).unwrap();
    recovered.enqueue(&scope, event(), 4).unwrap();
    assert!(recovered.next(&scope, &methods(), 4).is_none());
    assert!(
        Outbox::open(store)
            .unwrap()
            .next(&scope, &methods(), 4)
            .is_none()
    );
}

#[test]
fn failed_commits_leave_memory_and_disk_retryable() {
    let store = Arc::new(MemoryStore::default());
    let scope = scope();
    let mut outbox = Outbox::open(store.clone()).unwrap();
    outbox.activate(&scope, 0).unwrap();
    store.fail.store(true, Ordering::Relaxed);
    assert!(outbox.enqueue(&scope, event(), 1).is_err());
    assert!(outbox.next(&scope, &methods(), 1).is_none());
    store.fail.store(false, Ordering::Relaxed);
    outbox.enqueue(&scope, event(), 2).unwrap();
    let job = outbox.next(&scope, &methods(), 2).unwrap();
    store.fail.store(true, Ordering::Relaxed);
    assert!(outbox.complete(&job, None).is_err());
    assert_eq!(
        outbox.next(&scope, &methods(), 2).unwrap().entry.id,
        job.entry.id
    );
    assert_eq!(
        Outbox::open(store)
            .unwrap()
            .next(&scope, &methods(), 2)
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
    let old = outbox.next(&first, &methods(), 1).unwrap();
    outbox.snapshot("workspace", json!([]), false, 1).unwrap();
    let second = scope();
    assert!(outbox.next(&second, &methods(), 2).is_none());
    outbox.activate(&second, 2).unwrap();
    assert!(outbox.enqueue(&first, event(), 3).is_err());
    outbox.snapshot("workspace", json!([]), false, 3).unwrap();
    let current = outbox.next(&second, &methods(), 3).unwrap();
    assert_eq!(current.entry.params["revision"], 1);
    outbox.complete(&old, None).unwrap();
    assert_eq!(
        outbox.next(&second, &methods(), 4).unwrap().entry.id,
        current.entry.id
    );
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
    let first = outbox.next(&scope, &methods(), 0).unwrap();
    outbox.snapshot("agent", json!([]), false, 1).unwrap();
    let second = outbox.next(&scope, &methods(), 1).unwrap();
    assert_eq!(second.entry.params["revision"], 2);
    outbox.complete(&first, None).unwrap();
    assert_eq!(
        outbox.next(&scope, &methods(), 1).unwrap().entry.id,
        second.entry.id
    );
    outbox.complete(&second, None).unwrap();
    outbox.snapshot("agent", json!([]), false, 2).unwrap();
    assert!(outbox.next(&scope, &methods(), 2).is_none());
    outbox.snapshot("agent", json!([]), true, 3).unwrap();
    assert_eq!(
        outbox.next(&scope, &methods(), 3).unwrap().entry.params,
        second.entry.params
    );
}

#[test]
fn quarantine_and_expiration_do_not_block_other_events() {
    let mut outbox = Outbox::open(Arc::new(MemoryStore::default())).unwrap();
    let scope = scope();
    outbox.activate(&scope, 0).unwrap();
    outbox.enqueue(&scope, event(), 0).unwrap();
    let first = outbox.next(&scope, &methods(), 1).unwrap();
    outbox
        .complete(&first, Some("conflict".to_owned()))
        .unwrap();
    assert!(outbox.next(&scope, &methods(), 1).is_none());
    outbox.activate(&scope, RETENTION_MS).unwrap();
    outbox.enqueue(&scope, event(), RETENTION_MS).unwrap();
    assert!(outbox.next(&scope, &methods(), RETENTION_MS).is_some());
    assert!(outbox.next(&scope, &methods(), RETENTION_MS * 2).is_none());
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
    assert!(outbox.next(&scope, &methods(), 0).is_none());
}

#[test]
fn unsupported_events_do_not_hold_back_directory_snapshots() {
    let mut outbox = Outbox::open(Arc::new(MemoryStore::default())).unwrap();
    let scope = scope();
    outbox.activate(&scope, 0).unwrap();
    outbox.enqueue(&scope, event(), 0).unwrap();
    outbox.snapshot("agent", json!([]), false, 0).unwrap();
    let directory_only = [DIRECTORY.to_owned()];
    assert_eq!(
        outbox.next(&scope, &directory_only, 1).unwrap().method,
        DIRECTORY
    );
    assert_eq!(outbox.next(&scope, &methods(), 1).unwrap().method, EVENTS);
    assert!(outbox.next(&scope, &[], 1).is_none());
}

#[test]
fn a_fresh_link_retries_quarantined_events_and_snapshots_once() {
    let store = Arc::new(MemoryStore::default());
    let mut outbox = Outbox::open(store.clone()).unwrap();
    let scope = scope();
    outbox.activate(&scope, 0).unwrap();
    outbox.enqueue(&scope, event(), 0).unwrap();
    outbox.snapshot("agent", json!([]), false, 0).unwrap();
    for _ in 0..2 {
        let job = outbox.next(&scope, &methods(), 1).unwrap();
        outbox
            .complete(&job, Some("unauthorized".to_owned()))
            .unwrap();
    }
    assert!(outbox.next(&scope, &methods(), 1).is_none());
    outbox.snapshot("agent", json!([]), true, 1).unwrap();
    assert!(outbox.next(&scope, &methods(), 1).is_none());
    outbox.unblock().unwrap();
    let mut recovered = Outbox::open(store).unwrap();
    let retried = recovered.next(&scope, &methods(), 2).unwrap();
    assert_eq!(retried.method, EVENTS);
    recovered.complete(&retried, None).unwrap();
    let snapshot = recovered.next(&scope, &methods(), 2).unwrap();
    assert_eq!(snapshot.method, DIRECTORY);
    recovered.complete(&snapshot, None).unwrap();
    assert!(recovered.next(&scope, &methods(), 2).is_none());
}

#[test]
fn a_full_journal_reports_a_typed_error_and_keeps_existing_entries() {
    let mut outbox = Outbox::open(Arc::new(MemoryStore::default())).unwrap();
    let scope = scope();
    outbox.activate(&scope, 0).unwrap();
    for _ in 0..MAX_EVENTS {
        outbox
            .enqueue(
                &scope,
                json!({"eventId":Uuid::new_v4(),"method":"status.server_info","params":{}}),
                0,
            )
            .unwrap();
    }
    let error = outbox.enqueue(&scope, event(), 0).unwrap_err();
    assert!(error.is::<OutboxFull>());
    assert_eq!(outbox.document.events.len(), MAX_EVENTS);
}
