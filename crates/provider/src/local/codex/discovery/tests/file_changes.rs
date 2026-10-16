use super::*;

fn file_change() -> Value {
    json!({"type":"fileChange","id":"patch","status":"completed","changes":[
        {"path":"/repo/a.rs","kind":{"type":"update","movePath":null},
            "diff":"@@ -1 +1 @@\n-old_a\n+new_a"},
        {"path":"/repo/b.rs","kind":{"type":"add"},"diff":"+new_b"}
    ]})
}

fn entries(native: &Value) -> Vec<NativeItem> {
    timeline_items(
        native,
        "turn",
        "time",
        &crate::local::images::ImageStore::default(),
    )
    .unwrap()
}

#[test]
fn file_changes_keep_each_path_and_diff_in_independently_openable_cards() {
    let native = file_change();

    let mapped = entries(&native);

    assert_eq!(mapped.len(), 2);
    for (index, entry) in mapped.iter().enumerate() {
        assert_eq!(entry.item["name"], "fileChange");
        assert_eq!(entry.item["status"], "completed");
        assert_eq!(entry.item["detail"]["type"], "edit");
        assert_eq!(
            entry.item["detail"]["filePath"],
            native["changes"][index]["path"]
        );
        assert_eq!(
            entry.item["detail"]["unifiedDiff"],
            native["changes"][index]["diff"]
        );
    }
    assert_eq!(mapped[0].key, "native:turn:patch");
    assert_eq!(mapped[0].item["callId"], "patch");
    assert_eq!(mapped[1].key, "native:turn:patch:file:1");
    assert_eq!(mapped[1].item["callId"], "patch:file:1");
    assert_eq!(mapped, entries(&native));
}

#[test]
fn file_changes_preserve_single_file_identity_and_failure_status() {
    for status in ["completed", "failed", "declined"] {
        let mut native = file_change();
        native["status"] = json!(status);
        native["changes"].as_array_mut().unwrap().truncate(1);

        let mapped = entries(&native);

        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].key, "native:turn:patch");
        assert_eq!(mapped[0].item["callId"], "patch");
        assert_eq!(mapped[0].item["detail"]["filePath"], "/repo/a.rs");
        assert_eq!(
            mapped[0].item["status"],
            if status == "completed" {
                "completed"
            } else {
                "failed"
            }
        );
    }
    let mut native = file_change();
    native["status"] = json!("inProgress");
    assert!(entries(&native).is_empty());
}

#[test]
fn file_changes_bound_each_utf8_preview_without_losing_other_files() {
    let mut native = file_change();
    native["changes"][0]["diff"] = json!("界".repeat(128 * 1024));

    let mapped = entries(&native);

    assert_eq!(mapped.len(), 2);
    let preview = mapped[0].item["detail"]["unifiedDiff"].as_str().unwrap();
    assert!(preview.len() <= 128 * 1024);
    assert!(preview.contains("Output truncated"));
    assert_eq!(mapped[1].item["detail"]["filePath"], "/repo/b.rs");
    assert_eq!(mapped[1].item["detail"]["unifiedDiff"], "+new_b");
}

#[test]
fn file_changes_fall_back_without_publishing_partially_valid_paths() {
    for changes in [
        json!([]),
        json!([{"diff":"+missing path"}]),
        json!([{"path":"/repo/a.rs","diff":"+valid"},{"path":"","diff":"+invalid"}]),
    ] {
        let mut native = file_change();
        native["changes"] = changes;

        let mapped = entries(&native);

        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].item["detail"]["type"], "unknown");
        assert!(mapped[0].item["detail"].get("filePath").is_none());
    }
}
