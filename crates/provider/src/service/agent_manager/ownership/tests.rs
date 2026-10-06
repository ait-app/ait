use super::*;

#[test]
fn committed_runtime_publication_wakes_directory_readers_only_after_the_overlay_changes() {
    let changes = model::changes::Changes::default();
    let owners = Owners::default().with_changes(Some(changes.clone()));
    let mut notifications = changes.subscribe();
    let running = json!({"status":"running","final":{"status":"running",
        "activeTurn":{"turnId":"turn-1"}}});

    owners.pending(&["agent-1".into()]).unwrap();
    assert!(!notifications.has_changed().unwrap());
    owners.publish("agent-1", running.clone()).unwrap();
    assert!(notifications.has_changed().unwrap());
    assert_eq!(
        owners.snapshot("agent-1").unwrap()["activeTurn"]["turnId"],
        "turn-1"
    );
    notifications.borrow_and_update();
    owners.publish("agent-1", running).unwrap();
    assert!(!notifications.has_changed().unwrap());
    owners
        .publish(
            "agent-1",
            json!({"status":"idle","final":{"status":"idle","activeTurn":null}}),
        )
        .unwrap();
    assert!(notifications.has_changed().unwrap());
    assert!(owners.snapshot("agent-1").unwrap()["activeTurn"].is_null());
}

#[test]
fn native_and_host_aliases_share_one_owner_and_conflicting_writers_are_rejected() {
    let owners = Owners::default();
    let native = owners.native("codex", "native-thread").unwrap();
    let record: PersistedAgentRuntimeRecord = serde_json::from_value(json!({
        "id":"agent-1","provider":"codex","cwd":"/repo","workspaceId":"workspace",
        "createdAt":"2026-10-06T00:00:00Z","updatedAt":"2026-10-06T00:00:00Z",
        "persistence":{"provider":"codex","sessionId":"native-thread","nativeHandle":"native-alias"}
    }))
    .unwrap();
    owners.bind(&native, &record).unwrap();
    assert_eq!(owners.agent("agent-1").unwrap(), native);
    assert_eq!(owners.native("codex", "native-alias").unwrap(), native);
    assert_eq!(
        owners.bind("different-owner", &record),
        Err(ErrorCode::CatalogBusy)
    );
    owners.release(&native);
    owners.bind("new-owner", &record).unwrap();
    assert_eq!(owners.agent("agent-1").unwrap(), "new-owner");
}

#[test]
fn late_placement_stays_rejected_until_the_workspace_barrier_opens() {
    let owners = Owners::default();
    let lane = owners.agent("agent-1").unwrap();
    let owner = owners.owner(lane.clone());
    let barrier = owners.freeze(vec!["workspace:workspace".into()]).unwrap();
    for _ in 0..2 {
        assert_eq!(owner.place("workspace"), Err(ErrorCode::CatalogBusy));
    }
    drop(barrier);
    owner.place("workspace").unwrap();
    let scopes = BTreeSet::from(["workspace:workspace".into()]);
    assert_eq!(
        owners.related(&scopes).unwrap(),
        BTreeSet::from([lane.clone()])
    );
    let barrier = owners.freeze(scopes.into_iter().collect()).unwrap();
    let startup = owners.startup(&lane).unwrap();
    assert_eq!(startup.len(), 1);
    assert!(!startup[0].is_cancelled());
    // An already placed lane is fenced by the scheduler, so its in-flight factory can finish.
    owner.place("workspace").unwrap();
    drop(barrier);
    assert!(startup[0].is_cancelled());
    assert!(owners.startup(&lane).unwrap().is_empty());
}

#[test]
fn pending_admission_blocks_a_watch_before_the_native_writer_exists() {
    let owners = Owners::default();
    owners.pending(&["agent-1".into()]).unwrap();
    assert!(owners.snapshot("agent-1").is_none());
    let mut watch = owners.observe("agent-1", json!({"status":"idle"})).unwrap();
    assert_eq!(watch.borrow_and_update()["busy"], true);
    owners
        .owner("agent:agent-1".into())
        .admitting("agent-1")
        .unwrap();
    assert!(watch.has_changed().unwrap());
    assert_eq!(watch.borrow_and_update()["live"], true);
    owners
        .publish(
            "agent-1",
            json!({"status":"idle","busy":false,"final":{"status":"idle"}}),
        )
        .unwrap();
    assert_eq!(watch.borrow()["busy"], false);
    assert_eq!(owners.snapshot("agent-1").unwrap()["status"], "idle");
}
