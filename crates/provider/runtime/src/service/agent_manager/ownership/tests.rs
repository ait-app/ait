use super::*;

#[test]
fn waiters_receive_completion_fields_without_republishing_an_unchanged_directory_overlay() {
    let changes = model::changes::Changes::default();
    let owners = Owners::default().with_changes(Some(changes.clone()));
    let mut value = json!({"status":"idle","final":{"status":"idle"},
        "busy":true,"completionText":null});
    owners.publish("agent", value.clone()).unwrap();
    let notifications = changes.subscribe();
    let mut waiter = owners.observe("agent", Value::Null).unwrap();

    value["busy"] = json!(false);
    value["completionText"] = json!("Final response");
    owners.publish("agent", value.clone()).unwrap();

    assert!(waiter.has_changed().unwrap());
    assert_eq!(*waiter.borrow_and_update(), value);
    assert!(!notifications.has_changed().unwrap());
    assert_eq!(owners.snapshot("agent"), Some(json!({"status":"idle"})));
}

#[test]
fn runtime_overlay_clones_only_control_fields_without_losing_permissions() {
    let owners = Owners::default();
    let permissions = json!([{"id":"permission","input":{"path":"file"}}]);
    owners
        .publish(
            "agent",
            json!({"final":{
        "status":"running","requiresAttention":true,"attentionReason":"permission",
        "lastError":null,"activeTurn":{"turnId":"turn"},"providerUnavailable":false,
        "pendingPermissions":permissions,"runtimeInfo":{"extra":"x".repeat(1024*1024)},
        "config":{"opaque":"x".repeat(1024*1024)}}}),
        )
        .unwrap();
    let snapshot = owners.snapshot("agent").unwrap();
    assert_eq!(snapshot.as_object().unwrap().len(), 7);
    assert_eq!(snapshot["pendingPermissions"], permissions);
    assert_eq!(snapshot["activeTurn"]["turnId"], "turn");
    assert!(snapshot.get("runtimeInfo").is_none());
    assert!(snapshot.get("config").is_none());
    owners
        .publish(
            "agent",
            json!({"final":{"status":"idle","activeTurn":null}}),
        )
        .unwrap();
    assert_eq!(
        owners.snapshot("agent").unwrap(),
        json!({"status":"idle","activeTurn":null})
    );
    assert_eq!(snapshot["status"], "running");
}

#[test]
fn controls_are_reused_only_for_the_same_agent_native_identity_and_configuration() {
    let owners = Owners::default();
    let record: PersistedAgentRuntimeRecord = serde_json::from_value(json!({
        "id":"agent-1", "provider":"contract", "cwd":"/repo",
        "createdAt":"2026-10-31T00:00:00Z", "updatedAt":"2026-10-31T00:00:00Z",
        "config":{"model":"first", "systemPrompt":"private control config"},
        "persistence":{"provider":"contract","sessionId":"native-1"}
    }))
    .unwrap();
    let controls = json!({"features":[{"id":"fast","value":true}],
        "availableModes":[{"id":"native","label":"Native"}]});
    owners.publish_controls(&record, &controls);
    owners.publish_controls(&record, &controls);
    assert!(!format!("{owners:?}").contains("private control config"));
    let fallback = json!({"features":[], "availableModes":[]});
    let mut projected = fallback.clone();
    owners.restore_controls(&record, &mut projected);
    assert_eq!(projected, controls);

    let mut variants = Vec::new();
    let mut other = record.clone();
    other.id = "agent-2".to_owned();
    variants.push(other);
    let mut configured = record.clone();
    configured.config.as_mut().unwrap().model = Some("second".to_owned());
    variants.push(configured);
    let mut discovered = record.clone();
    discovered.runtime_info = Some(
        serde_json::from_value(json!({
            "provider":"contract", "extra":{"availableModes":[]}
        }))
        .unwrap(),
    );
    variants.push(discovered);
    let mut replaced = record.clone();
    replaced.persistence.as_mut().unwrap().session_id = "native-2".to_owned();
    variants.push(replaced);
    let mut changed_provider = record.clone();
    changed_provider.provider = "other".to_owned();
    variants.push(changed_provider);
    for changed in variants {
        let mut projected = fallback.clone();
        owners.restore_controls(&changed, &mut projected);
        assert_eq!(projected, fallback, "stale controls masked updated facts");
    }
    let lane = owners.agent(&record.id).unwrap();
    owners.release(&lane);
    let mut projected = fallback.clone();
    owners.restore_controls(&record, &mut projected);
    assert_eq!(
        projected, fallback,
        "a released owner retained control facts"
    );
}

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
    assert_eq!(owners.startup(&lane).unwrap(), [] as [CancellationToken; 0]);
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

#[test]
fn released_and_forgotten_observations_are_dropped_once_no_waiter_remains() {
    let owners = Owners::default();
    let committed = json!({"status":"idle","final":{"status":"idle"},"completionText":"done"});
    for id in ["agent-1", "agent-2"] {
        let lane = owners.agent(id).unwrap();
        owners.publish(id, committed.clone()).unwrap();
        assert_eq!(lane, format!("agent:{id}"));
    }
    let waiter = owners.observe("agent-1", Value::Null).unwrap();

    owners.release("agent:agent-1");
    assert!(owners.snapshot("agent-1").is_some());
    assert!(owners.snapshot("agent-2").is_some());
    drop(waiter);
    owners.release("agent:agent-1");
    assert!(owners.snapshot("agent-1").is_none());
    assert!(owners.snapshot("agent-2").is_some());

    let waiter = owners.observe("agent-2", Value::Null).unwrap();
    owners.forget("agent-2");
    assert!(owners.snapshot("agent-2").is_some());
    drop(waiter);
    owners.forget("agent-2");
    assert!(owners.snapshot("agent-2").is_none());
}
