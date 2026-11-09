use super::*;
use std::time::Duration;

#[test]
fn native_goals_validate_operations_objectives_and_exact_positive_budgets() {
    for op in ["get", "resume", "pause", "drop"] {
        assert_eq!(
            validate("goal", &json!({"op":op}), "/tmp").unwrap(),
            json!({"op":op})
        );
    }
    assert_eq!(
        validate(
            "goal",
            &json!({"op":"create","objective":"Finish the task","token_budget":"1200"}),
            "/tmp"
        )
        .unwrap(),
        json!({"op":"create","objective":"Finish the task","token_budget":1200})
    );
    for arguments in [
        json!({"op":"create"}),
        json!({"op":"create","objective":" "}),
        json!({"op":"create","objective":"task","token_budget":"0"}),
        json!({"op":"create","objective":"task","token_budget":"9007199254740992"}),
        json!({"op":"create","objective":"task","token_budget":"1.5"}),
        json!({"op":"get","objective":"ignored"}),
        json!({"op":"edit"}),
    ] {
        assert!(validate("goal", &arguments, "/tmp").is_err());
    }
}

#[test]
fn actions_reject_unknown_controls_and_foreign_children() {
    for action in [
        "prompt",
        "switch_session",
        "set_host_tools",
        "set_event_filter",
    ] {
        assert!(validate(action, &json!({}), "/tmp").is_err());
    }
    assert!(
        validate(
            "login",
            &json!({"providerId":"provider","secret":"value"}),
            "/tmp"
        )
        .is_err()
    );
    assert!(validate("set_cache_warming", &json!({"mode":"invalid"}), "/tmp").is_err());
    assert_eq!(
        validate("live_mute", &json!({"muted":"false"}), "/tmp").unwrap(),
        json!({"muted":false})
    );
    assert!(validate("live_mute", &json!({"muted":false}), "/tmp").is_err());
    assert!(validate_child(&json!({"subagents":[{"id":"owned"}]}), &json!("foreign")).is_err());
    validate_child(&json!({"subagents":[{"id":"owned"}]}), &json!("owned")).unwrap();
}

#[test]
fn dictation_uses_a_fresh_host_generated_capture_identity() {
    let first = validate("dictation_start", &json!({}), "/tmp").unwrap();
    let second = validate("dictation_start", &json!({}), "/tmp").unwrap();
    uuid::Uuid::parse_str(first["captureId"].as_str().unwrap()).unwrap();
    assert_ne!(first["captureId"], second["captureId"]);
    assert!(validate("dictation_start", &json!({"captureId":"foreign"}), "/tmp").is_err());
}

#[tokio::test]
async fn history_receipts_wait_for_reconciliation_and_close_releases_waiters() {
    let mut pending = Pending::default();
    let mut receipt = pending.insert("change".to_owned(), "navigate_tree");
    assert!(pending.changing_history());
    pending.capture(&json!({"id":"change","command":"navigate_tree","success":true,"data":{}}));
    assert!(
        tokio::time::timeout(Duration::from_millis(1), &mut receipt)
            .await
            .is_err()
    );
    let refresh = pending.take_refresh().unwrap();
    pending.prepared(refresh);
    assert!(
        tokio::time::timeout(Duration::from_millis(1), &mut receipt)
            .await
            .is_err()
    );
    pending.committed();
    receipt.await.unwrap();
    assert!(!pending.changing_history());
    let receipt = pending.insert("close".to_owned(), "login");
    pending.close();
    assert_eq!(receipt.await.unwrap_err(), AgentSessionError::Failed);
}

#[tokio::test]
async fn native_receipts_correlate_and_rejections_leave_other_controls_usable() {
    let mut pending = Pending::default();
    let first = pending.insert("one".to_owned(), "login");
    let second = pending.insert("two".to_owned(), "get_session_stats");
    assert!(!pending.capture(&json!({"id":"foreign","command":"login","success":true})));
    assert!(pending.capture(
        &json!({"id":"two","command":"get_session_stats","success":true,"data":{"tokens":2}})
    ));
    assert!(
        second.await.unwrap()["text"]
            .as_str()
            .unwrap()
            .contains("tokens")
    );
    assert!(pending.capture(&json!({"id":"one","command":"login","success":false})));
    assert!(first.await.unwrap()["error"].is_string());
    pending.check_capacity().unwrap();
}

#[test]
fn vibe_control_is_advertised_and_only_accepts_declared_fields_per_operation() {
    let advertised = catalog()
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["id"] == "vibe_control");
    assert!(advertised);
    // Toggle accepts an optional boolean and nothing else.
    assert_eq!(
        validate(
            "vibe_control",
            &json!({"operation":"toggle","enabled":"true"}),
            "/tmp"
        )
        .unwrap(),
        json!({"operation":"toggle","enabled":true})
    );
    assert!(
        validate(
            "vibe_control",
            &json!({"operation":"toggle","prompt":"x"}),
            "/tmp"
        )
        .is_err()
    );
    // Spawn carries the worker class and prompt; wait carries an ID list and timeout.
    assert!(
        validate(
            "vibe_control",
            &json!({"operation":"spawn","cli":"fast","prompt":"inspect the parser"}),
            "/tmp"
        )
        .is_ok()
    );
    assert!(
        validate(
            "vibe_control",
            &json!({"operation":"spawn","cli":"slow","prompt":"x"}),
            "/tmp"
        )
        .is_err()
    );
    assert_eq!(
        validate(
            "vibe_control",
            &json!({"operation":"wait","sessions":"a,b","timeout":"30"}),
            "/tmp"
        )
        .unwrap(),
        json!({"operation":"wait","sessions":["a","b"],"timeout":30.0})
    );
    // Worker identity is an Ait subagent ID and is required for send/kill.
    assert!(
        validate(
            "vibe_control",
            &json!({"operation":"send","message":"x"}),
            "/tmp"
        )
        .is_err()
    );
    assert!(
        validate(
            "vibe_control",
            &json!({"operation":"kill","sessionId":"worker-id"}),
            "/tmp"
        )
        .is_ok()
    );
    for bad in [
        json!({"operation":"destroy"}),
        json!({"operation":"list","sessionId":"x"}),
    ] {
        assert!(validate("vibe_control", &bad, "/tmp").is_err(), "{bad}");
    }
}

#[tokio::test]
async fn history_choice_receipts_do_not_cut_json_when_native_messages_are_long() {
    let entries: Vec<_> = (0..8)
        .map(|index| {
            json!({"id":format!("entry-{index}"),"type":"message",
            "message":{"role":"user","content":"Native task text ".repeat(2000)}})
        })
        .collect();
    let event = json!({"id":"history","command":"get_entries","success":true,
        "data":{"entries":entries,"leafId":"entry-7"}});
    let mut pending = Pending::default();
    let receipt = pending.insert("history".to_owned(), "get_entries");

    assert!(pending.capture(&event));
    let response = receipt.await.unwrap();
    let text = response["text"].as_str().unwrap();
    let choices: Value = serde_json::from_str(text).unwrap();

    assert!(text.len() < 32768);
    assert_eq!(choices["entries"].as_array().unwrap().len(), 8);
    assert_eq!(choices["entries"][7]["id"], "entry-7");
    assert_eq!(choices["entries"][7]["role"], "user");
    assert_eq!(choices["omitted"], 0);
    assert!(
        event["data"]["entries"][0]["message"]["content"]
            .as_str()
            .unwrap()
            .len()
            > 30000
    );
}
