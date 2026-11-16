use super::*;

/// The `async-result` OMP 18.18.8 delivers when a background bash job settles.
const BASH_NOTICE: &str = "<system-notice>\nBackground job bg_1 (sleep 2; echo async-done) has \
    completed. Resume your work using the result below.\nasync-done\nWall time: 2.15 seconds\n\
    </system-notice>";
/// The `async-result` OMP 18.18.8 delivers when a background task settles.
const TASK_NOTICE: &str = "<system-notice>\nBackground job Prober has completed. Resume your \
    work using the result below.\n<task-result id=\"Prober\" agent=\"task\" status=\"completed\" \
    duration=\"50ms\">\n\n<output>\nchild done\n</output>\n</task-result>\n\nProber is now idle\n\
    </system-notice>";

fn custom_message(custom_type: &str, details: Value) -> Value {
    let mut message = json!({"role":"custom","customType":custom_type,"display":true,
        "timestamp":1000});
    message["details"] = details;
    message
}

#[test]
fn background_notices_are_notifications_that_keep_the_job_output() {
    let bash = custom(
        &custom_message("async-result", json!({})),
        BASH_NOTICE,
        "key",
    );
    assert_eq!(
        bash,
        json!({"type":"notification","level":"info","message":"Background job bg_1 (sleep 2; \
            echo async-done) has completed. Resume your work using the result below.\n\
            async-done\nWall time: 2.15 seconds"})
    );
    let task = custom(
        &custom_message("async-result", json!({})),
        TASK_NOTICE,
        "key",
    );
    assert_eq!(
        task["message"],
        "Background job Prober completed\nchild done\nProber is now idle"
    );
    assert_eq!(task["level"], "info");
    for (status, level) in [
        ("failed", "error"),
        ("Error", "error"),
        ("cancelled", "warning"),
        ("stopped", "warning"),
    ] {
        let text =
            format!("<system-notice>\n<task-result id=\u{201c}job\u{201d} status='{status}'>");
        let notice = system_notice(&text).unwrap();
        assert_eq!(notice["level"], level);
        assert_eq!(notice["message"], format!("Background job job {status}"));
    }
    assert_eq!(
        system_notice("<system-notice></system-notice>").unwrap()["message"],
        "System notice"
    );
    assert!(system_notice("text <system-notice>").is_none());
    assert!(task_result("<task-results id=\"x\">").is_none());
}

#[test]
fn advisor_notes_are_one_row_with_severity_tags() {
    let notes = json!({"notes":[
        {"note":"Rename it","severity":"nit"},
        {"note":" Breaks the build ","severity":"blocker","advisor":"rust"},
        {"note":"Unclear","severity":"urgent"},
        {"note":"  "}]});
    let row = custom(&custom_message("advisor", notes), "raw advice", "key");
    assert_eq!(row["name"], "advisor");
    assert_eq!(row["status"], "completed");
    assert!(row["callId"].as_str().unwrap().starts_with("omp-advisor:"));
    assert_eq!(
        row["detail"],
        json!({"type":"plain_text","label":"Advisor · 3 notes · 1 blocker","icon":"brain",
            "text":"[nit] Rename it\n\n[blocker] [rust] Breaks the build\n\nUnclear"})
    );
    assert_eq!(row["metadata"]["noteCount"], 3);
    assert_eq!(row["metadata"]["blockerCount"], 1);
    let plain = custom(&custom_message("advisor", Value::Null), "raw advice", "key");
    assert_eq!(plain["detail"]["label"], "Advisor");
    assert_eq!(plain["detail"]["text"], "raw advice");
    // The same advice keeps its call id, so live output and replay agree.
    assert_eq!(plain["callId"], row["callId"]);
    let named = json!({"role":"custom","customType":"advisor","id":"a1"});
    assert_eq!(custom(&named, "x", "key")["callId"], "omp-advisor:a1");
}

#[test]
fn other_custom_messages_are_plain_rows_named_after_their_type() {
    let row = custom(
        &custom_message("compat-vendor-alpha", json!({"k":1})),
        "output",
        "native:omp:s:custom:1000:0",
    );
    assert_eq!(
        row,
        json!({"type":"tool_call","callId":"native:omp:s:custom:1000:0",
            "name":"compat-vendor-alpha","status":"completed",
            "detail":{"type":"plain_text","text":"output"},
            "metadata":{"synthetic":true,"customType":"compat-vendor-alpha","details":{"k":1}},
            "error":null})
    );
    let large = json!({"blob":"x".repeat(MAX_DETAILS_BYTES)});
    let untyped = custom(&custom_message(" ", large), "output", "key");
    assert_eq!(untyped["name"], "custom-message");
    assert!(untyped["metadata"].get("details").is_none());
}

#[test]
fn notices_goals_retries_and_fallbacks_become_stable_rows() {
    // OMP 18.18.8 `--warn-cost`.
    let notice =
        json!({"type":"notice","level":"warning","message":"Cost warning","source":"cost-gate"});
    assert_eq!(
        telemetry(&notice),
        Some(json!({"type":"notification","level":"warning","message":"Cost warning"}))
    );
    assert!(telemetry(&json!({"type":"notice","level":"debug","message":"x"})).is_none());

    let goal = telemetry(
        &json!({"type":"goal_updated","goal":{"id":"g1","objective":"Ship",
        "status":"active","tokensUsed":10,"timeUsedSeconds":3},"state":{"mode":"auto"}}),
    )
    .unwrap();
    assert_eq!(goal["callId"], "omp-goal:g1");
    assert_eq!(goal["name"], "omp_goal_updated");
    assert_eq!(goal["detail"]["label"], "OMP goal active");
    assert_eq!(
        goal["detail"]["text"],
        "Ship\nStatus: active\nTokens used: 10\nTime used: 3s\nMode: auto"
    );
    let cleared = telemetry(&json!({"type":"goal_updated","goal":null})).unwrap();
    assert_eq!(cleared["detail"]["text"], "OMP goal cleared.");
    assert_eq!(
        cleared["callId"],
        telemetry(&json!({"type":"goal_updated"})).unwrap()["callId"]
    );

    let start = telemetry(
        &json!({"type":"auto_retry_start","attempt":2,"maxAttempts":3,
        "delayMs":1500,"errorMessage":"overloaded","errorId":7}),
    )
    .unwrap();
    assert_eq!(start["callId"], "omp-auto-retry:2");
    assert_eq!(start["status"], "running");
    assert_eq!(start["detail"]["label"], "OMP retry 2/3");
    assert_eq!(start["detail"]["text"], "Retrying in 1.5s: overloaded");
    assert_eq!(start["metadata"]["errorId"], 7);
    let failed = telemetry(&json!({"type":"auto_retry_end","attempt":2,"success":false,
        "finalError":"gave up"}))
    .unwrap();
    assert_eq!(failed["callId"], start["callId"]);
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["error"], "gave up");
    let recovered =
        telemetry(&json!({"type":"auto_retry_end","attempt":1,"success":true})).unwrap();
    assert_eq!(recovered["detail"]["text"], "Retry recovered.");
    assert!(recovered["error"].is_null());
    // The fixture's legacy keep-alive frame lacks the native fields and is dropped.
    assert!(telemetry(&json!({"type":"auto_retry_start","attempt":1})).is_none());

    let applied = json!({"type":"retry_fallback_applied","from":"a/x","to":"b/y","role":"default",
        "reason":"rate limit"});
    let first = telemetry(&applied).unwrap();
    assert_eq!(first["callId"], telemetry(&applied).unwrap()["callId"]);
    assert_eq!(first["detail"]["text"], "a/x -> b/y");
    let succeeded =
        telemetry(&json!({"type":"retry_fallback_succeeded","model":"b/y","role":"default"}))
            .unwrap();
    assert_eq!(
        succeeded["detail"]["label"],
        "OMP fallback succeeded for default"
    );
    assert_ne!(succeeded["callId"], first["callId"]);
    assert!(telemetry(&json!({"type":"agent_start"})).is_none());
}

#[test]
fn compaction_events_are_loading_and_completed_rows() {
    // OMP 18.18.8 shapes; `compaction_*` is Paseo's older lifecycle.
    assert_eq!(
        telemetry(&json!({"type":"auto_compaction_start","reason":"threshold","action":"handoff"})),
        Some(json!({"type":"compaction","status":"loading","trigger":"auto"}))
    );
    assert_eq!(
        telemetry(
            &json!({"type":"auto_compaction_end","action":"handoff","aborted":false,
            "result":{"summary":"s","tokensBefore":100_010}})
        ),
        Some(
            json!({"type":"compaction","status":"completed","trigger":"auto","preTokens":100_010})
        )
    );
    assert_eq!(
        telemetry(&json!({"type":"manual_compaction_end","aborted":false,"skipped":true})),
        Some(json!({"type":"compaction","status":"completed","trigger":"manual"}))
    );
    assert_eq!(
        telemetry(&json!({"type":"compaction_start","reason":"manual"})).unwrap()["trigger"],
        "manual"
    );
    assert_eq!(
        telemetry(&json!({"type":"compaction_end"})).unwrap()["trigger"],
        "auto"
    );
    assert!(manual_compaction(
        &json!({"type":"manual_compaction_start"})
    ));
    assert!(manual_compaction(&json!({"type":"compaction_end"})));
    assert!(!manual_compaction(&json!({"type":"auto_compaction_end"})));
    assert_eq!(delay_text(250), "250ms");
    assert_eq!(delay_text(2000), "2s");
}

#[test]
fn todo_lists_drop_abandoned_items_and_name_blockers() {
    // OMP 18.18.8 todo tool `details.phases` after `drop` and `block`.
    let phases = json!([
        {"name":"A","tasks":[{"content":"x","status":"in_progress"},
            {"content":"y","status":"abandoned"},{"content":"w","status":"completed"}]},
        {"name":"B","tasks":[{"content":"z","status":"blocked","blocker":"waiting on api"},
            {"content":"v","status":"blocked"}]}]);
    assert_eq!(
        todo_phases(&phases),
        Some(json!({"type":"todo","items":[
            {"text":"x","status":"in_progress","completed":false,"group":{"id":"phase:0","title":"A"}},
            {"text":"w","status":"completed","completed":true,"group":{"id":"phase:0","title":"A"}},
            {"text":"z (blocked: waiting on api)","status":"pending","completed":false,"group":{"id":"phase:1","title":"B"}},
            {"text":"v (blocked)","status":"pending","completed":false,"group":{"id":"phase:1","title":"B"}}]}))
    );
    assert_eq!(
        todo_reminder(&json!({"type":"todo_reminder","todos":[{"content":"two",
            "status":"in_progress"}],"attempt":1,"maxAttempts":3})),
        Some(
            json!({"type":"todo","items":[{"text":"two","status":"in_progress","completed":false}]})
        )
    );
    assert_eq!(
        todo_phases(&json!([{"name":"A","tasks":[{"content":"y","status":"abandoned"}]}])),
        Some(json!({"type":"todo","items":[]}))
    );
    for malformed in [
        json!(null),
        json!([{"tasks":[]}]),
        json!([{"name":"A","tasks":[{"content":"x","status":"unknown"}]}]),
    ] {
        assert!(todo_phases(&malformed).is_none(), "{malformed}");
    }
    assert!(todo_reminder(&json!({"type":"todo_reminder"})).is_none());
    assert_eq!(
        todo_phases(&json!([])),
        Some(json!({"type":"todo","items":[]}))
    );
    assert_eq!(
        todo_phases(&json!([{"name":"Empty","tasks":[]}])),
        Some(json!({"type":"todo","items":[]}))
    );
}

#[test]
fn native_todo_phase_groups_keep_duplicate_names_separate_and_preserve_numbered_content() {
    let projected = todo_phases(&json!([
        {"name":"Build","tasks":[{"content":"1.2 Implement","status":"pending"}]},
        {"name":"Build","tasks":[{"content":"2. Verify","status":"completed"}]}
    ]))
    .unwrap();
    assert_eq!(projected["items"][0]["text"], "1.2 Implement");
    assert_eq!(projected["items"][1]["text"], "2. Verify");
    assert_eq!(
        projected["items"][0]["group"],
        json!({"id":"phase:0","title":"Build"})
    );
    assert_eq!(
        projected["items"][1]["group"],
        json!({"id":"phase:1","title":"Build"})
    );
}

#[test]
fn todo_group_replication_is_bounded_before_cloning_native_strings() {
    for phases in [
        json!([{"name":"x".repeat(8193),"tasks":[{"content":"One","status":"pending"}]}]),
        json!([{"name":"Phase","tasks":vec![json!({"content":"One","status":"pending"});1025]}]),
        json!([{"name":"x".repeat(8192),"tasks":vec![json!({"content":"One","status":"pending"});17]}]),
        json!([{"name":"\n".repeat(8192),"tasks":vec![json!({"content":"One","status":"pending"});9]}]),
    ] {
        assert!(todo_phases(&phases).is_none());
    }
}
