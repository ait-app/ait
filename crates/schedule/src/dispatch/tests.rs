use chrono::{DateTime, Utc};
use domain::storage::schedule::Error;
use model::ErrorCode;
use serde_json::json;

use super::error_code;
use crate::engine::{
    Engine,
    tests::{Memory, input},
};

#[test]
fn running_target_updates_report_a_distinct_rpc_error() {
    let now: DateTime<Utc> = "2026-10-19T14:00:00Z".parse().unwrap();
    let mut engine = Engine::open(Box::new(Memory::default()), now).unwrap();
    let created = engine
        .request("schedule.create.request", input(), now)
        .unwrap();
    let id = created["schedule"]["id"].as_str().unwrap();
    let _ = engine.begin(id, true, now).unwrap();
    let failure = engine
        .request(
            "schedule.update.request",
            json!({"scheduleId":id,"prompt":"Updated instructions","target":input()["target"]}),
            now,
        )
        .unwrap_err();
    let code = error_code(failure);

    assert_eq!(serde_json::to_value(code).unwrap(), "schedule_running");
    assert_eq!(
        code.message(),
        "This schedule is running. Wait for the current run to finish before editing it."
    );
    assert_eq!(engine.inspect(id).unwrap().prompt, "hello");
}

#[test]
fn other_schedule_failures_keep_the_generic_rpc_error() {
    for failure in [
        Error::Invalid,
        Error::NotFound,
        Error::Conflict,
        Error::Storage,
    ] {
        assert_eq!(error_code(failure), ErrorCode::ScheduleRequestFailed);
    }
}
