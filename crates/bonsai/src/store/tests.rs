use super::{AskRecord, InputRecord, StatusUpdate, Store, StoreError};
use crate::testing::dispatch;
use crate::wire::{Execution, ReasonCode, RunState};

fn update(status: RunState, at: i64) -> StatusUpdate {
    StatusUpdate {
        status,
        reason_code: None,
        reason_detail: None,
        final_text: None,
        at,
    }
}

#[test]
fn runs_are_recorded_once_and_read_back() {
    let store = Store::memory().expect("store");
    let run = dispatch("r_1");
    assert!(store.insert_run(&run, "e-1", 10).expect("insert"));
    assert!(!store.insert_run(&run, "e-2", 11).expect("duplicate"));
    let record = store.run("r_1").expect("read").expect("present");
    assert_eq!(record.dispatch, run);
    assert_eq!(record.status, RunState::Claimed);
    assert_eq!(record.epoch, "e-1");
    assert_eq!(record.next_seq, 0);
    assert!(record.execution.is_none());
    assert!(store.run("r_missing").expect("read").is_none());
}

#[test]
fn states_only_move_forward_and_terminal_states_stick() {
    let store = Store::memory().expect("store");
    store.insert_run(&dispatch("r_1"), "e", 1).expect("insert");
    assert!(
        store
            .advance("r_1", &update(RunState::Running, 2))
            .expect("advance")
    );
    assert!(
        !store
            .advance("r_1", &update(RunState::Claimed, 3))
            .expect("back")
    );
    assert!(
        !store
            .advance("r_1", &update(RunState::Running, 4))
            .expect("same")
    );
    let failed = StatusUpdate {
        reason_code: Some(ReasonCode::ProviderError),
        reason_detail: Some("boom".to_owned()),
        ..update(RunState::Failed, 5)
    };
    assert!(store.advance("r_1", &failed).expect("fail"));
    assert!(
        !store
            .advance("r_1", &update(RunState::Cancelled, 6))
            .expect("terminal")
    );
    let record = store.run("r_1").expect("read").expect("present");
    assert_eq!(record.status, RunState::Failed);
    assert_eq!(record.reason_code, Some(ReasonCode::ProviderError));
    assert_eq!(record.status_at, 5);
    assert!(store.open_runs().expect("open runs").is_empty());
    assert!(
        !store
            .advance("r_none", &update(RunState::Running, 1))
            .expect("unknown")
    );
}

#[test]
fn execution_agent_cancel_and_mode_are_stored() {
    let store = Store::memory().expect("store");
    store.insert_run(&dispatch("r_1"), "e", 1).expect("insert");
    let execution = Execution {
        provider: "claude".to_owned(),
        model: Some("sonnet".to_owned()),
        approvals: true,
        bonsai_write: false,
    };
    store.set_execution("r_1", &execution).expect("execution");
    store.set_agent("r_1", "agent-1").expect("agent");
    store.request_cancel("r_1").expect("cancel");
    store.set_mode("r_1", Some("plan")).expect("mode");
    let record = store.run("r_1").expect("read").expect("present");
    assert_eq!(record.execution, Some(execution));
    assert_eq!(record.agent_id.as_deref(), Some("agent-1"));
    assert!(record.cancel_requested);
    assert_eq!(record.mode.as_deref(), Some("plan"));
    assert_eq!(store.open_runs().expect("open").len(), 1);
}

#[test]
fn events_append_in_order_and_replay_by_range() {
    let store = Store::memory().expect("store");
    store.insert_run(&dispatch("r_1"), "e1", 1).expect("insert");
    let batch: Vec<String> = (0..3).map(|seq| format!("{{\"seq\":{seq}}}")).collect();
    store.append("r_1", "e1", 0, &batch, None).expect("append");
    store
        .append(
            "r_1",
            "e1",
            3,
            &["{\"seq\":3}".to_owned()],
            Some(("ait-e", 41)),
        )
        .expect("append more");
    let record = store.run("r_1").expect("read").expect("present");
    assert_eq!(record.next_seq, 4);
    assert_eq!(record.ait_cursor, Some(("ait-e".to_owned(), 41)));
    assert_eq!(store.events("r_1", "e1", 1, 3).expect("range"), batch[1..3]);
    assert!(store.events("r_1", "e1", 4, 4).expect("empty").is_empty());
    assert_eq!(store.events("r_1", "e1", 2, 9), Err(StoreError::Corrupt));
    assert!(
        store
            .append("r_1", "e1", 3, &["{}".to_owned()], None)
            .is_err(),
        "seq reuse"
    );
}

#[test]
fn rotation_starts_a_new_generation_from_zero() {
    let store = Store::memory().expect("store");
    store.insert_run(&dispatch("r_1"), "e1", 1).expect("insert");
    store
        .append("r_1", "e1", 0, &["{}".to_owned()], Some(("a", 1)))
        .expect("append");
    store.rotate("r_1", "e2").expect("rotate");
    let record = store.run("r_1").expect("read").expect("present");
    assert_eq!((record.epoch.as_str(), record.next_seq), ("e2", 0));
    assert_eq!(record.ait_cursor, None);
    assert_eq!(store.events("r_1", "e1", 0, 1), Err(StoreError::Corrupt));
}

#[test]
fn tombstones_inputs_and_asks_round_trip() {
    let store = Store::memory().expect("store");
    assert!(!store.is_buried("r_9").expect("lookup"));
    store.bury("r_9", 5).expect("bury");
    store.bury("r_9", 6).expect("bury twice");
    assert!(store.is_buried("r_9").expect("lookup"));
    let input = |id: &str| InputRecord {
        input_id: id.to_owned(),
        message_id: format!("m-{id}"),
        text: "hi".to_owned(),
        state: "queued".to_owned(),
        by: Some("github:2".to_owned()),
        login: Some("member".to_owned()),
    };
    assert!(store.insert_input("r_1", &input("b")).expect("insert"));
    assert!(store.insert_input("r_1", &input("a")).expect("insert"));
    assert!(!store.insert_input("r_1", &input("b")).expect("duplicate"));
    store.set_input_state("r_1", "b", "sent").expect("state");
    let inputs = store.inputs("r_1").expect("inputs");
    assert_eq!(
        inputs
            .iter()
            .map(|i| i.input_id.as_str())
            .collect::<Vec<_>>(),
        ["b", "a"]
    );
    assert_eq!(inputs[0].state, "sent");
    assert_eq!(inputs[0].by.as_deref(), Some("github:2"));
    let ask = AskRecord {
        ask_id: "q1".to_owned(),
        spec: "{}".to_owned(),
        state: "resolving".to_owned(),
        resolving_by: Some("github:2".to_owned()),
        resolving_effect: Some("allow".to_owned()),
    };
    store.put_ask("r_1", &ask).expect("put");
    assert_eq!(store.asks("r_1").expect("asks"), vec![ask]);
}

#[test]
fn state_survives_reopening_the_file() {
    let directory = tempfile::tempdir().expect("temp dir");
    let path = directory.path().join("bonsai/runtime.sqlite3");
    {
        let store = Store::open(&path).expect("open");
        store.insert_run(&dispatch("r_1"), "e1", 1).expect("insert");
        store
            .append("r_1", "e1", 0, &["{}".to_owned()], None)
            .expect("append");
    }
    let store = Store::open(&path).expect("reopen");
    assert_eq!(
        store.run("r_1").expect("read").expect("present").next_seq,
        1
    );
    assert_eq!(store.events("r_1", "e1", 0, 1).expect("events"), ["{}"]);
}

#[test]
fn ends_cover_every_run() {
    let store = Store::memory().expect("store");
    store.insert_run(&dispatch("r_1"), "e1", 1).expect("insert");
    store.insert_run(&dispatch("r_2"), "e2", 2).expect("insert");
    store
        .append("r_2", "e2", 0, &["{}".to_owned()], None)
        .expect("append");
    let mut ends = store.ends().expect("ends");
    ends.sort();
    assert_eq!(
        ends,
        vec![
            ("r_1".to_owned(), "e1".to_owned(), 0),
            ("r_2".to_owned(), "e2".to_owned(), 1)
        ]
    );
}

#[test]
fn the_owner_is_remembered_and_replaced() {
    let store = Store::memory().expect("store");
    assert_eq!(store.owner().expect("read"), None);
    let first = crate::wire::Person {
        id: "github:1".to_owned(),
        login: Some("a".to_owned()),
    };
    let second = crate::wire::Person {
        id: "github:2".to_owned(),
        login: None,
    };
    store.set_owner(&first).expect("write");
    store.set_owner(&second).expect("replace");
    assert_eq!(store.owner().expect("read"), Some(second));
}

#[cfg(unix)]
#[test]
fn the_database_and_its_directory_are_private_to_the_owner() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().expect("temp dir");
    let directory = root.path().join("bonsai");
    std::fs::create_dir(&directory).expect("a pre-existing, too open directory");
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let path = directory.join("runtime.sqlite3");

    let store = Store::open(&path).expect("open");
    store.insert_run(&dispatch("r_1"), "e1", 0).expect("write");

    let mode = |path: &std::path::Path| {
        std::fs::metadata(path)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(mode(&directory), 0o700);
    assert_eq!(mode(&path), 0o600);
    // WAL mode keeps both journal files open while the store is.
    for journal in ["runtime.sqlite3-wal", "runtime.sqlite3-shm"] {
        let journal = directory.join(journal);
        assert!(journal.exists(), "{}", journal.display());
        assert_eq!(mode(&journal) & 0o077, 0, "{}", journal.display());
    }
}
