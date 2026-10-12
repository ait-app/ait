//! Workspace ownership, finite triggers and binding migration.
use super::*;

#[test]
fn new_tasks_require_workspace_ownership_and_lists_filter_by_it() {
    let (mut engine, store) = fixture();
    let mut params = input();
    params.as_object_mut().unwrap().remove("workspaceId");
    assert_eq!(
        engine
            .request("schedule.create.request", params, now())
            .unwrap_err(),
        Error::Invalid
    );
    assert!(store.0.lock().unwrap().0.is_empty());
    let first = create(&mut engine);
    let mut params = input();
    params["workspaceId"] = json!("wks-other");
    engine
        .request("schedule.create.request", params, now())
        .unwrap();
    let listed = engine
        .request(
            "schedule.list.request",
            json!({"workspaceId":"wks-test"}),
            now(),
        )
        .unwrap();
    assert_eq!(listed["schedules"].as_array().unwrap().len(), 1);
    assert_eq!(listed["schedules"][0]["id"], first);
    assert_eq!(listed["schedules"][0]["workspaceId"], "wks-test");
    assert_eq!(
        engine
            .request("schedule.list.request", json!({"workspaceId":null}), now())
            .unwrap_err(),
        Error::Invalid
    );
}

#[test]
fn binding_is_immutable_and_legacy_records_can_be_bound_once() {
    let (mut engine, store) = fixture();
    let id = create(&mut engine);
    assert_eq!(
        engine
            .request(
                "schedule.update.request",
                json!({"scheduleId":id,"workspaceId":"wks-other"}),
                now()
            )
            .unwrap_err(),
        Error::Invalid
    );
    assert_eq!(
        engine.inspect(&id).unwrap().workspace_id.as_deref(),
        Some("wks-test")
    );
    store.0.lock().unwrap().0[0].workspace_id = None;
    let mut engine = Engine::open(Box::new(store), now()).unwrap();
    engine
        .request(
            "schedule.update.request",
            json!({"scheduleId":id,"workspaceId":"wks-migrated"}),
            now(),
        )
        .unwrap();
    assert_eq!(
        engine.inspect(&id).unwrap().workspace_id.as_deref(),
        Some("wks-migrated")
    );
    assert_eq!(
        engine
            .request(
                "schedule.update.request",
                json!({"scheduleId":id,"workspaceId":"wks-test"}),
                now()
            )
            .unwrap_err(),
        Error::Invalid
    );
}

#[test]
fn targets_can_be_changed_before_execution_but_not_during_it() {
    let (mut engine, _) = fixture();
    let id = create(&mut engine);
    let target = json!({"type":"new-agent","config":{"provider":"codex"}});
    engine
        .request(
            "schedule.update.request",
            json!({"scheduleId":id,"target":target}),
            now(),
        )
        .unwrap();
    assert!(matches!(
        engine.inspect(&id).unwrap().target,
        Target::NewAgent { .. }
    ));
    let (_, run) = engine.begin(&id, true, now()).unwrap();
    assert_eq!(
        engine
            .request(
                "schedule.update.request",
                json!({"scheduleId":id,"target":input()["target"]}),
                now()
            )
            .unwrap_err(),
        Error::Conflict
    );
    engine
        .finish(&id, &run, true, Outcome::default(), now())
        .unwrap();
    engine
        .request(
            "schedule.update.request",
            json!({"scheduleId":id,"target":input()["target"]}),
            now(),
        )
        .unwrap();
    assert!(matches!(
        engine.inspect(&id).unwrap().target,
        Target::Agent { .. }
    ));
}

#[test]
fn once_triggers_at_the_requested_time_and_settles_terminally() {
    let (mut engine, _) = fixture();
    let at = now() + chrono::Duration::hours(2);
    let mut params = input();
    params["cadence"] = json!({"type":"once","at":at});
    let id = engine
        .request("schedule.create.request", params, now())
        .unwrap()["schedule"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        engine
            .due(at - chrono::Duration::milliseconds(1))
            .unwrap()
            .is_empty()
    );
    assert_eq!(engine.due(at).unwrap(), vec![id.clone()]);
    let (record, run) = engine.begin(&id, false, at).unwrap();
    assert_eq!(record.runs[0].workspace_id.as_deref(), Some("wks-test"));
    engine
        .finish(&id, &run, false, Outcome::default(), at)
        .unwrap();
    assert_eq!(engine.inspect(&id).unwrap().status, Status::Completed);
    assert!(engine.inspect(&id).unwrap().next_run_at.is_none());
    assert_eq!(engine.begin(&id, true, at).unwrap_err(), Error::Conflict);
}

#[test]
fn restart_runs_a_missed_once_trigger_but_never_repeats_an_interrupted_one() {
    let (mut engine, store) = fixture();
    let at = now() + chrono::Duration::minutes(1);
    let mut params = input();
    params["cadence"] = json!({"type":"once","at":at});
    let id = engine
        .request("schedule.create.request", params, now())
        .unwrap()["schedule"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let later = at + chrono::Duration::hours(1);
    let mut engine = Engine::open(Box::new(store.clone()), later).unwrap();
    assert_eq!(engine.due(later).unwrap(), vec![id.clone()]);
    engine.begin(&id, false, later).unwrap();
    let mut engine = Engine::open(Box::new(store), later).unwrap();
    assert!(engine.due(later).unwrap().is_empty());
    let record = engine.inspect(&id).unwrap();
    assert_eq!(record.status, Status::Completed);
    assert_eq!(record.runs[0].status, RunStatus::Failed);
}

#[test]
fn interval_first_trigger_and_resume_respect_future_start_time() {
    let (mut engine, _) = fixture();
    let start = now() + chrono::Duration::hours(2);
    let mut params = input();
    params["startAt"] = json!(start);
    let id = engine
        .request("schedule.create.request", params, now())
        .unwrap()["schedule"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(engine.inspect(&id).unwrap().next_run_at, Some(start));
    engine
        .request("schedule.pause.request", json!({"scheduleId":id}), now())
        .unwrap();
    engine
        .request("schedule.resume.request", json!({"scheduleId":id}), now())
        .unwrap();
    assert_eq!(engine.inspect(&id).unwrap().next_run_at, Some(start));
    assert!(
        engine
            .due(start - chrono::Duration::seconds(1))
            .unwrap()
            .is_empty()
    );
    let (_, run) = engine.begin(&id, false, start).unwrap();
    engine
        .finish(&id, &run, false, Outcome::default(), start)
        .unwrap();
    assert_eq!(
        engine.inspect(&id).unwrap().next_run_at,
        Some(start + chrono::Duration::minutes(1))
    );
}

#[test]
fn invalid_trigger_combinations_never_publish_records() {
    for patch in [
        json!({"cadence":{"type":"once","at":now()}}),
        json!({"cadence":{"type":"once","at":now()+chrono::Duration::hours(1)},"startAt":now()+chrono::Duration::hours(2)}),
        json!({"startAt":now()+chrono::Duration::hours(1),"runOnCreate":true}),
        json!({"startAt":now()-chrono::Duration::hours(1)}),
        json!({"runOnCreate":false,"expiresAt":now()+chrono::Duration::seconds(30)}),
        json!({"workspaceId":" "}),
    ] {
        let (mut engine, store) = fixture();
        let mut params = input();
        params
            .as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        assert_eq!(
            engine
                .request("schedule.create.request", params, now())
                .unwrap_err(),
            Error::Invalid
        );
        assert!(store.0.lock().unwrap().0.is_empty());
    }
}
