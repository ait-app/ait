use super::supported;

#[test]
fn early_command_ack_requires_a_native_lifecycle_receipt_capability() {
    let missing = serde_json::json!({});
    assert!(!super::command_lifecycle("2.1.3", &missing));
    assert!(!super::command_lifecycle("2.1.3-local", &missing));
    assert!(super::command_lifecycle("1.18.4", &missing));
    assert!(super::command_lifecycle("2.0.25", &missing));
    let implemented = serde_json::json!({"_meta":{"opencode/command-lifecycle":1}});
    assert!(super::command_lifecycle("2.1.3", &implemented));
    let unknown = serde_json::json!({"_meta":{"opencode/command-lifecycle":2}});
    assert!(!super::command_lifecycle("2.1.3", &unknown));
}

#[test]
fn supported_releases_require_a_fixed_v2_catalog_and_preserve_v1() {
    for version in ["1.18.4", "2.0.25", "2.0.26", "2.0.27", "2.1.0", "2.12.0"] {
        assert!(supported(version), "{version}");
    }
    for version in [
        "0.9.0", "2.0.0", "2.0.20", "2.0.24", "3.0.0", "2.0", "invalid",
    ] {
        assert!(!supported(version), "{version}");
    }
}
