use super::*;

#[test]
fn creation_receipts_preserve_saved_settings_without_transient_native_ui() {
    let snapshot = json!({"id":"agent","features":[{"id":"modelRole.smol","value":"native"}],
        "providerUi":{"editor":{"text":"private draft"},"link":{"url":"https://example.com/oauth"}}});
    let durable = creation_snapshot(snapshot);
    assert!(durable.get("providerUi").is_none());
    assert_eq!(durable["features"][0]["id"], "modelRole.smol");
}
