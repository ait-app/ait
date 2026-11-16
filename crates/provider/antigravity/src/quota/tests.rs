use super::*;

fn response() -> Value {
    json!({"status":"SUCCESS","num_turns":0,"command":{"name":"usage","data":{
        "groups":[{"name":"Gemini Models","buckets":[{"id":"gemini-5h","window":"5h",
            "remaining_fraction":0.25,"reset_time":"2026-10-14T12:00:00Z"}]}]}}})
}

#[test]
fn quota_projection_preserves_native_windows_without_exporting_unrelated_fields() {
    let mut native = response();
    native["command"]["data"]["token"] = json!("private-token");
    native["response"] = json!("private-response");
    for (fraction, used, tone) in [
        (0.0, 100.0, "danger"),
        (0.25, 75.0, "warning"),
        (1.0, 0.0, "ok"),
    ] {
        native["command"]["data"]["groups"][0]["buckets"][0]["remaining_fraction"] =
            json!(fraction);
        let report = project(&native).unwrap();
        assert_eq!(report["status"], "available");
        assert_eq!(report["windows"][0]["usedPct"], used);
        assert_eq!(report["windows"][0]["tone"], tone);
        assert_eq!(report["windows"][0]["resetsAt"], "2026-10-14T12:00:00Z");
        assert_eq!(report["windows"][0]["summary"], true);
        assert!(report["planLabel"].is_null());
        assert!(report.get("accountLabel").is_none());
        assert!(!report.to_string().contains("private"));
    }
    let bucket = &mut native["command"]["data"]["groups"][0]["buckets"][0];
    bucket["window"] = json!("monthly");
    bucket.as_object_mut().unwrap().remove("reset_time");
    let report = project(&native).unwrap();
    assert_eq!(report["windows"][0]["label"], "Gemini Models · monthly");
    assert!(report["windows"][0]["resetsAt"].is_null());
}

#[test]
fn quota_projection_rejects_non_command_results_and_malformed_native_facts() {
    for (pointer, value) in [
        ("/status", json!("ERROR")),
        ("/command/name", json!("other")),
        ("/num_turns", json!(1)),
        ("/command/data/groups", Value::Null),
        ("/command/data/groups/0/name", json!(" ")),
        ("/command/data/groups/0/name", json!("x".repeat(257))),
        ("/command/data/groups/0/buckets", Value::Null),
        ("/command/data/groups/0/buckets/0/id", json!("bad\nid")),
        ("/command/data/groups/0/buckets/0/window", json!("")),
        (
            "/command/data/groups/0/buckets/0/remaining_fraction",
            json!(-0.1),
        ),
        (
            "/command/data/groups/0/buckets/0/remaining_fraction",
            json!(1.1),
        ),
        (
            "/command/data/groups/0/buckets/0/remaining_fraction",
            json!("75%"),
        ),
        (
            "/command/data/groups/0/buckets/0/remaining_fraction",
            Value::Null,
        ),
        (
            "/command/data/groups/0/buckets/0/reset_time",
            json!("invalid"),
        ),
    ] {
        let mut native = response();
        *native.pointer_mut(pointer).unwrap() = value;
        assert_eq!(
            project(&native),
            Err(AgentSessionError::Failed),
            "{pointer}"
        );
    }
    let mut duplicate = response();
    let group = duplicate["command"]["data"]["groups"][0].clone();
    duplicate["command"]["data"]["groups"]
        .as_array_mut()
        .unwrap()
        .push(group);
    assert!(project(&duplicate).is_err());
}

#[test]
fn empty_quotas_are_not_an_authentication_failure_and_projection_is_bounded() {
    let mut native = response();
    native["command"]["data"]["groups"] = json!([]);
    let report = project(&native).unwrap();
    assert_eq!(report["status"], "unavailable");
    assert_eq!(report["problem"]["kind"], "no_quota");
    assert!(!report.to_string().contains("Sign in"));
    assert_eq!(report["windows"], json!([]));

    let template = response()["command"]["data"]["groups"][0].clone();
    native["command"]["data"]["groups"] = json!(vec![template.clone(); 65]);
    assert!(project(&native).is_err());
    native["command"]["data"]["groups"] = json!([template]);
    let buckets = (0..512)
        .map(|index| {
            json!({"id":format!("window-{index}"),
        "window":"weekly","remaining_fraction":0.5})
        })
        .collect::<Vec<_>>();
    native["command"]["data"]["groups"][0]["buckets"] = json!(buckets);
    assert_eq!(
        project(&native).unwrap()["windows"]
            .as_array()
            .unwrap()
            .len(),
        512
    );
    native["command"]["data"]["groups"][0]["buckets"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"overflow","window":"5h","remaining_fraction":0.5}));
    assert!(project(&native).is_err());
}
