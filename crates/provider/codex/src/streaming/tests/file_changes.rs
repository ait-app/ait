use super::*;

#[test]
fn file_changes_keep_live_completed_and_replayed_cards_in_sync() {
    let mut stream = Stream::default();
    let mut native = json!({"type":"fileChange","id":"patch","status":"inProgress","changes":[
        {"path":"/repo/a.rs","diff":"@@ -1 +1 @@\n-old\n+new"},
        {"path":"/repo/b.rs","diff":"+second"}
    ]});
    let started = json!({"turnId":"turn","item":native});
    let first = stream.progress("item/started", &started).unwrap();
    let running: Vec<_> = first
        .into_iter()
        .chain(stream.events.drain(..))
        .map(|event| item(Some(event)))
        .collect();
    assert_eq!(running.len(), 2);
    assert!(stream.progress("item/started", &started).unwrap().is_none());
    let timeline = provider::testing::memory_timeline().unwrap();
    for (index, entry) in running.iter().enumerate() {
        assert_eq!(entry.item["status"], "running");
        provider::testing::progress_timeline(
            &timeline,
            "agent",
            "codex",
            &format!("started-{index}"),
            entry,
        )
        .unwrap();
    }

    native["status"] = json!("completed");
    let completed = discovery::timeline_items(
        &native,
        "turn",
        "time",
        &provider_native::images::ImageStore::default(),
    )
    .unwrap();
    assert_eq!(completed.len(), running.len());
    for (live, finished) in running.iter().zip(&completed) {
        assert_eq!(live.key, finished.key);
        assert_eq!(live.item["detail"], finished.item["detail"]);
        assert_eq!(finished.item["status"], "completed");
    }
    provider::testing::append_timeline(&timeline, "agent", "codex", &completed).unwrap();
    let before = provider::testing::timeline_rows(&timeline, "agent").unwrap();
    assert_eq!(
        provider::testing::projected_timeline_items(&timeline, "agent")
            .unwrap()
            .len(),
        2
    );
    provider::testing::reconcile_timeline(&timeline, "agent", "codex", &completed).unwrap();
    assert_eq!(
        provider::testing::timeline_rows(&timeline, "agent").unwrap(),
        before
    );
    assert!(stream.complete(&native).unwrap());
    assert!(!stream.complete(&native).unwrap());
    assert!(
        stream
            .progress("item/fileChange/outputDelta", &params("patch", "late"))
            .unwrap()
            .is_none()
    );
    assert_eq!(stream.tool_rows, 0);
}

#[test]
fn file_changes_reuse_the_initial_placeholder_when_paths_arrive_on_completion() {
    let mut stream = Stream::default();
    let placeholder = item(
        stream
            .progress(
                "item/started",
                &json!({"turnId":"turn",
        "item":{"id":"patch","type":"fileChange","changes":[]}}),
            )
            .unwrap(),
    );
    let native = json!({"type":"fileChange","id":"patch","status":"completed","changes":[
        {"path":"/repo/a.rs","diff":"+first"},{"path":"/repo/b.rs","diff":"+second"}
    ]});

    let completed = discovery::timeline_items(
        &native,
        "turn",
        "time",
        &provider_native::images::ImageStore::default(),
    )
    .unwrap();

    assert_eq!(completed.len(), 2);
    assert_eq!(placeholder.key, completed[0].key);
    let timeline = provider::testing::memory_timeline().unwrap();
    provider::testing::progress_timeline(&timeline, "agent", "codex", "started", &placeholder)
        .unwrap();
    provider::testing::append_timeline(&timeline, "agent", "codex", &completed).unwrap();
    let stored = provider::testing::projected_timeline_items(&timeline, "agent").unwrap();
    assert_eq!(stored.len(), 2);
    assert!(stored.iter().all(|item| item["status"] == "completed"));
}
