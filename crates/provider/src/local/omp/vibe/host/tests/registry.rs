use super::*;
use crate::local::omp::vibe::host::registry::Usage;

fn record(id: usize, status: &str) -> Record {
    Record {
        child: NativeSubagent {
            id: id.to_string(),
            parent_id: "parent".into(),
            cwd: "/project".into(),
            persistence: None,
            descriptor: json!({"status":status, "updatedAt":format!("{id:04}")}),
        },
        cli: "fast".into(),
        config: StoredAgentConfig::default(),
        answer: "done".into(),
    }
}

#[test]
fn completed_workers_are_evicted_in_age_order_instead_of_exhausting_lifetime_capacity() {
    let mut registry = Registry::default();
    for id in 0..MAX_WORKERS * 3 {
        registry.make_room(false).unwrap();
        registry
            .records
            .insert(id.to_string(), record(id, "completed"));
        assert!(registry.records.len() <= MAX_WORKERS);
    }
    assert_eq!(registry.records.len(), MAX_WORKERS);
    for id in 0..MAX_WORKERS * 2 {
        assert!(!registry.records.contains_key(&id.to_string()));
    }
    for id in MAX_WORKERS * 2..MAX_WORKERS * 3 {
        assert!(registry.records.contains_key(&id.to_string()));
    }
}

#[test]
fn active_starting_and_in_use_workers_are_never_evicted() {
    let host = host();
    {
        let mut registry = host.shared.registry.lock().unwrap();
        registry.records.insert("0".into(), record(0, "completed"));
        for id in 1..MAX_WORKERS - 1 {
            registry
                .records
                .insert(id.to_string(), record(id, "running"));
        }
        registry.starting.insert("new".into(), None);
    }
    let first = Usage::acquire(host.shared.clone(), vec!["0".into()]).unwrap();
    let second = Usage::acquire(host.shared.clone(), vec!["0".into()]).unwrap();
    assert_eq!(
        host.shared.registry.lock().unwrap().make_room(false),
        Err(AgentSessionError::Busy)
    );
    drop(first);
    assert_eq!(
        host.shared.registry.lock().unwrap().make_room(false),
        Err(AgentSessionError::Busy)
    );
    drop(second);
    let mut registry = host.shared.registry.lock().unwrap();
    registry.make_room(false).unwrap();
    assert!(!registry.records.contains_key("0"));
    assert_eq!(registry.records.len(), MAX_WORKERS - 2);
    assert!(registry.starting.contains_key("new"));
}

#[cfg(unix)]
#[tokio::test]
async fn worker_activity_is_committed_before_receipt_and_stale_events_do_not_restore_evicted_records()
 {
    let (_root, client, spec) = crate::local::omp::tests::fixture();
    let mut transport = crate::local::omp::session::probe(&client, &spec.cwd, None)
        .await
        .unwrap();
    let mut host = host();
    host.shared
        .registry
        .lock()
        .unwrap()
        .records
        .insert("0".into(), record(0, "completed"));
    let usage = Usage::acquire(host.shared.clone(), vec!["0".into()]).unwrap();
    assert!(worker::publish(&host.shared, 0, &record(0, "running")).await);
    drop(usage);
    assert!(
        host.running(),
        "activity must not depend on the parent's next poll"
    );
    assert!(worker::publish(&host.shared, 0, &record(0, "completed")).await);
    host.shared.registry.lock().unwrap().records.remove("0");
    assert!(!worker::publish(&host.shared, 0, &record(0, "running")).await);
    host.poll(transport.writer().unwrap());
    assert!(host.shared.registry.lock().unwrap().records.is_empty());
    transport.close().await.unwrap();
}
