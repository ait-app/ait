use super::*;

mod command_actions;

#[test]
fn oversized_mcp_text_keeps_history_readable_and_replays_idempotently() {
    let text = "界\n\"\\".repeat(128 * 1024);
    let native = json!({"id":"call","type":"mcpToolCall","server":"fixture","tool":"read",
        "status":"completed","arguments":{"path":"file"},
        "result":{"content":[{"type":"text","text":text}],"isError":false}});
    let images = crate::local::images::ImageStore::default();
    let items = timeline_items(&native, "turn", "time", &images).unwrap();
    let timeline = crate::storage::timeline::Timeline::memory().unwrap();

    let epoch = timeline.reconcile("agent", "codex", &items).unwrap();
    assert_eq!(timeline.reconcile("agent", "codex", &items).unwrap(), epoch);
    let (_, rows) = timeline.read("agent").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].entry.key, "native:turn:call");
    assert_eq!(rows[0].entry.item["name"], "fixture.read");
    assert_eq!(rows[0].entry.item["detail"]["input"], native["arguments"]);
    assert!(
        rows[0].entry.item["detail"]["output"]
            .as_str()
            .unwrap()
            .contains("Output truncated")
    );
    assert_eq!(native["result"]["content"][0]["text"], text);
}

#[test]
fn generated_images_and_mcp_images_have_stable_sanitized_history() {
    let root = tempfile::tempdir().unwrap();
    let images = crate::local::images::ImageStore::new(root.path().join("images"));
    let generation =
        json!({"id":"image","type":"imageGeneration","status":"completed","result":"aGVsbG8="});
    let first = timeline_items(&generation, "turn", "time", &images).unwrap();
    assert_eq!(
        first,
        timeline_items(&generation, "turn", "time", &images).unwrap()
    );
    assert_eq!(first[0].item["type"], "assistant_message");
    let mcp = json!({"id":"tool","type":"mcpToolCall","status":"completed","result":{"content":[
        {"type":"image","mimeType":"image/png","data":"aGVsbG8="},{"type":"text","text":"caption"}]}});
    let items = timeline_items(&mcp, "turn", "time", &images).unwrap();
    assert_eq!(items.len(), 2);
    assert!(!items[0].item.to_string().contains("aGVsbG8="));
    assert_eq!(items[1].key, "native:turn:tool:image:0");
    assert_eq!(items[1].item["text"], first[0].item["text"]);
}

#[test]
fn mcp_screenshot_metadata_does_not_exhaust_the_timeline() {
    let native = json!({"id":"call","type":"mcpToolCall","server":"cua_repl","tool":"js",
        "status":"completed","arguments":{"code":"await tab.screenshot()"},
        "result":{"content":[{"type":"text","text":"Done"}],"isError":false,
            "_meta":{"codex/toolSurface":{"screenshot":{"url":"x".repeat(320 * 1024)}}}}});
    let items = timeline_items(
        &native,
        "turn",
        "time",
        &crate::local::images::ImageStore::default(),
    )
    .unwrap();
    assert_eq!(items.len(), 1);
    assert!(items[0].item["detail"]["output"].get("_meta").is_none());
    assert_eq!(
        items[0].item["detail"]["output"]["content"][0]["text"],
        "Done"
    );

    let timeline = crate::storage::timeline::Timeline::memory().unwrap();
    timeline.append("agent", "codex", &items).unwrap();
}

#[test]
fn completed_mcp_tool_uses_the_same_name_and_input_as_its_running_card() {
    let native = json!({"id":"call","type":"mcpToolCall","server":"cua_repl","tool":"js",
        "status":"completed","arguments":{"code":"await tab.goto('https://example.com')"},
        "result":{"content":[{"type":"text","text":"Done"}]}});
    let items = timeline_items(
        &native,
        "turn",
        "time",
        &crate::local::images::ImageStore::default(),
    )
    .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].item["callId"], "call");
    assert_eq!(items[0].item["name"], "cua_repl.js");
    assert_eq!(
        items[0].item["detail"],
        json!({"type":"unknown",
        "input":{"code":"await tab.goto('https://example.com')"},
        "output":{"content":[{"type":"text","text":"Done"}]}})
    );
    let mut failed = native;
    failed["status"] = json!("failed");
    failed["error"] = json!({"message":"Access denied"});
    let failure = timeline_items(
        &failed,
        "turn",
        "time",
        &crate::local::images::ImageStore::default(),
    )
    .unwrap();
    assert_eq!(failure[0].item["status"], "failed");
    assert_eq!(failure[0].item["error"], json!({"message":"Access denied"}));
}

#[test]
fn native_items_keep_stable_source_identity_and_supported_display_shapes() {
    for (native, expected) in [
        (
            json!({"type":"userMessage","id":"u","content":[{"type":"text","text":"hello"}]}),
            "user_message",
        ),
        (
            json!({"type":"agentMessage","id":"a","text":"reply"}),
            "assistant_message",
        ),
        (
            json!({"type":"reasoning","id":"r","summary":["think"]}),
            "reasoning",
        ),
        (json!({"type":"contextCompaction","id":"c"}), "compaction"),
        (json!({"type":"plan","id":"p","text":"plan"}), "tool_call"),
        (
            json!({"type":"commandExecution","id":"t","command":"pwd","status":"completed"}),
            "tool_call",
        ),
    ] {
        let item = timeline_item(&native, "turn", "time").unwrap().unwrap();
        assert_eq!(item.item["type"], expected);
        assert!(item.key.starts_with("native:turn:"));
    }
    assert!(
        timeline_item(
            &json!({"type":"commandExecution","id":"t","status":"inProgress"}),
            "turn",
            "time"
        )
        .unwrap()
        .is_none()
    );
    assert!(
        timeline_item(
            &json!({"type":"agentMessage","text":"no identity"}),
            "turn",
            "time"
        )
        .is_err()
    );
    assert!(model(&json!({})).is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn native_discovery_and_read_only_history_use_real_stdio_without_model_calls() {
    use crate::ports::agent_session::{AgentClient, AgentTurnEvent};
    let fixture = crate::test_support::Fixture::new();
    let client = fixture.client();
    let details = client
        .discover(fixture.cwd.to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(details.models[0]["id"], "offline-model");
    assert!(fixture.requests().iter().any(|request| {
        request["method"] == "model/list" && request["params"]["includeHidden"] == false
    }));
    assert_eq!(
        details
            .modes
            .iter()
            .map(|mode| (mode["id"].as_str(), mode["icon"].as_str()))
            .collect::<Vec<_>>(),
        vec![
            (Some("auto"), Some("Shield")),
            (Some("full-access"), Some("ShieldOff")),
        ]
    );
    assert_eq!(details.features[0]["icon"], "zap");
    assert_eq!(details.features[0]["tooltip"], "Toggle fast mode");
    let mut session = client.create_session(&fixture.spec()).await.unwrap();
    let handle = session.persistence().unwrap();
    session
        .start_turn("history", &fixture.spec().config)
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if matches!(
                session.poll_turn().unwrap(),
                Some(AgentTurnEvent::Completed(_))
            ) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    session.close().await.unwrap();
    let history = client
        .history(&handle, fixture.cwd.to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].item["text"], "history");
    assert!(
        fixture
            .requests()
            .iter()
            .any(|r| r["method"] == "thread/read" && r["params"]["includeTurns"] == true)
    );
}
