use super::*;
use serde_json::json;

#[test]
fn validates_bounded_identifiers() {
    assert_eq!(text(&json!({"id":"native-id"}), "id").unwrap(), "native-id");
    for value in [
        json!({}),
        json!({"id":""}),
        json!({"id":"bad\n"}),
        json!({"id":"x".repeat(1025)}),
    ] {
        assert!(text(&value, "id").is_err());
    }
}
