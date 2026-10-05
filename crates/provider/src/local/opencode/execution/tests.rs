use super::*;
use serde_json::json;

#[test]
fn persisted_idle_settles_v2_when_execution_events_are_not_exposed() {
    let user = json!({"type":"user","time":{"created":10}});
    for (status, expected) in [
        ("succeeded", Outcome::Completed),
        ("failed", Outcome::Failed),
        ("interrupted", Outcome::Interrupted),
    ] {
        let info = json!({"outcome":status});
        let idle = json!({"type":"idle","time":{"created":11},"outcome":status});
        let history = [user.clone(), idle.clone()];
        assert_eq!(
            outcome(Version::V2, &info, None, &history).unwrap(),
            Some(expected)
        );
        assert_eq!(
            outcome(Version::V2, &info, None, &[idle, user.clone()]).unwrap(),
            None
        );
        assert!(outcome(Version::V2, &Value::Null, None, &history).is_err());
    }
    for idle in [
        json!({"type":"idle","outcome":"succeeded"}),
        json!({"type":"idle","time":{"created":9},"outcome":"succeeded"}),
        json!({"type":"idle","time":{"created":11},"outcome":"unknown"}),
    ] {
        assert!(
            outcome(
                Version::V2,
                &json!({"outcome":"succeeded"}),
                None,
                &[user.clone(), idle]
            )
            .is_err()
        );
    }
    assert_eq!(
        outcome(Version::V2, &json!({"outcome":"succeeded"}), None, &[user]).unwrap(),
        None
    );
}

#[test]
fn latest_durable_execution_must_settle_after_the_latest_input() {
    let history = [json!({"type":"user","time":{"created":10}})];
    let info = json!({"outcome":"succeeded"});
    let mut event = json!({"type":"session.execution.succeeded","created":11});
    assert_eq!(
        outcome(Version::V2, &info, Some(&event), &history).unwrap(),
        Some(Outcome::Completed)
    );
    event["created"] = json!(9);
    assert_eq!(
        outcome(Version::V2, &info, Some(&event), &history).unwrap(),
        None
    );
    event["created"] = json!(11);
    event["type"] = json!("session.execution.started");
    assert_eq!(
        outcome(Version::V2, &info, Some(&event), &history).unwrap(),
        None
    );
    event["type"] = json!("session.execution.interrupted");
    event["data"] = json!({"reason":"shutdown"});
    assert_eq!(
        outcome(Version::V2, &info, Some(&event), &history).unwrap(),
        None
    );
    event["data"] = json!({"reason":"user"});
    assert!(outcome(Version::V2, &info, Some(&event), &history).is_err());
    assert_eq!(
        outcome(
            Version::V2,
            &json!({"outcome":"interrupted"}),
            Some(&event),
            &history
        )
        .unwrap(),
        Some(Outcome::Interrupted)
    );
}

#[test]
fn legacy_idle_does_not_complete_a_newer_queued_input() {
    let history = [
        json!({"info":{"role":"assistant"}}),
        json!({"info":{"role":"user"}}),
    ];
    assert_eq!(
        outcome(Version::V1, &Value::Null, None, &history).unwrap(),
        None
    );
    assert_eq!(
        outcome(Version::V1, &Value::Null, None, &history[..1]).unwrap(),
        Some(Outcome::Completed)
    );
}
