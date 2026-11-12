use super::*;

#[test]
fn task_snapshots_filter_malformed_rows_and_keep_earlier_snapshots_immutable() {
    let mut tasks = Tasks::default();
    tasks.observe(&call("list", "TaskList", json!({}))).unwrap();
    let initial = tasks
        .observe(&result(
            "list",
            json!({"tasks":[
                {"taskId":"one","subject":"Original","status":"unknown"},
                {"id":"deleted","subject":"Gone","status":"deleted"},
                {"subject":"Missing identity"},
                {"id":"missing-subject"}
            ]}),
        ))
        .unwrap();
    assert_eq!(
        initial[0].1["items"],
        json!([{"id":"one","text":"Original","status":"pending","completed":false}])
    );
    tasks
        .observe(&call(
            "rename",
            "TaskUpdate",
            json!({"taskId":"one","subject":"Renamed","activeForm":"Renaming"}),
        ))
        .unwrap();
    let changed = tasks.observe(&result("rename", json!({}))).unwrap();
    assert_eq!(changed[0].1["items"][0]["text"], "Renamed");
    assert_eq!(changed[0].1["items"][0]["activeForm"], "Renaming");
    assert_eq!(initial[0].1["items"][0]["text"], "Original");
    for (index, name, input, output) in [
        (0, "TaskCreate", json!({"subject":"No identity"}), json!({})),
        (1, "TaskCreate", json!({}), json!({"taskId":"no-subject"})),
        (2, "TaskUpdate", json!({}), json!({})),
        (3, "TaskUpdate", json!({"taskId":"absent"}), json!({})),
        (4, "TaskList", json!({}), json!({"tasks":false})),
    ] {
        let id = format!("invalid-{index}");
        tasks.observe(&call(&id, name, input)).unwrap();
        assert_eq!(
            tasks.observe(&result(&id, output)).unwrap(),
            [] as [(String, Value); 0]
        );
        assert_eq!(tasks.snapshot(), changed[0].1);
    }
    assert_eq!(
        tasks.observe(&result("", json!({}))).unwrap(),
        [] as [(String, Value); 0]
    );
    tasks
        .observe(&call(
            "error",
            "TaskUpdate",
            json!({"taskId":"one","status":"deleted"}),
        ))
        .unwrap();
    let mut failed = result("error", json!({}));
    failed["message"]["content"][0]["is_error"] = json!(true);
    assert_eq!(tasks.observe(&failed).unwrap(), [] as [(String, Value); 0]);
    assert_eq!(tasks.snapshot(), changed[0].1);
}

#[test]
fn unbounded_task_lists_and_pending_calls_are_rejected_without_losing_saved_tasks() {
    let mut tasks = Tasks::default();
    tasks
        .observe(&call(
            "saved",
            "TodoWrite",
            json!({"todos":[{"content":"Keep"}]}),
        ))
        .unwrap();
    let before = tasks.snapshot();
    let oversized = json!({"todos":vec![json!({"content":"too many"});1025]});
    assert_eq!(
        tasks.observe(&call("large", "TodoWrite", oversized)),
        Err(AgentSessionError::Failed)
    );
    assert_eq!(tasks.snapshot(), before);
    for index in 0..1022 {
        tasks
            .observe(&call(&format!("pending-{index}"), "TaskList", json!({})))
            .unwrap();
    }
    assert_eq!(
        tasks.observe(&call("overflow", "TaskList", json!({}))),
        Err(AgentSessionError::Failed)
    );
    assert_eq!(tasks.snapshot(), before);
}

fn call(id: &str, name: &str, input: Value) -> Value {
    let mut message =
        json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":id,"name":name}]}});
    message["message"]["content"][0]["input"] = input;
    message
}

fn result(id: &str, result: Value) -> Value {
    let mut message =
        json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":id}]}});
    message["toolUseResult"] = result;
    message
}

#[test]
fn task_tools_preserve_ids_status_updates_deletion_and_do_not_apply_failed_or_duplicate_results() {
    let mut tasks = Tasks::default();
    let legacy = call(
        "legacy",
        "TodoWrite",
        json!({"todos":[{"content":"Prior task","status":"in_progress","activeForm":"Working"}]}),
    );
    let snapshot = tasks.observe(&legacy).unwrap();
    assert_eq!(snapshot[0].1["items"][0]["id"], "legacy:0");
    assert_eq!(tasks.observe(&legacy).unwrap(), [] as [(String, Value); 0]);
    tasks.observe(&call("list", "TaskList", json!({}))).unwrap();
    assert_eq!(
        tasks.observe(&result("list", json!({"tasks":[]}))).unwrap()[0].1["items"],
        json!([])
    );
    tasks
        .observe(&call(
            "create",
            "TaskCreate",
            json!({"subject":"Build","activeForm":"Building"}),
        ))
        .unwrap();
    let created = result("create", json!({"task":{"id":"1"}}));
    assert_eq!(
        tasks.observe(&created).unwrap()[0].1["items"][0]["text"],
        "Build"
    );
    assert_eq!(tasks.observe(&created).unwrap(), [] as [(String, Value); 0]);
    tasks
        .observe(&call(
            "update",
            "TaskUpdate",
            json!({"taskId":"1","status":"completed"}),
        ))
        .unwrap();
    assert_eq!(
        tasks
            .observe(&result("update", json!({"success":true})))
            .unwrap()[0]
            .1["items"][0]["completed"],
        true
    );
    tasks
        .observe(&call(
            "failed",
            "TaskUpdate",
            json!({"taskId":"1","status":"deleted"}),
        ))
        .unwrap();
    assert_eq!(
        tasks
            .observe(&result("failed", json!({"success":false})))
            .unwrap(),
        [] as [(String, Value); 0]
    );
    assert_eq!(tasks.items.len(), 1);
    tasks
        .observe(&call(
            "delete",
            "TaskUpdate",
            json!({"taskId":"1","status":"deleted"}),
        ))
        .unwrap();
    assert_eq!(
        tasks.observe(&result("delete", json!({}))).unwrap()[0].1["items"],
        json!([])
    );
}
