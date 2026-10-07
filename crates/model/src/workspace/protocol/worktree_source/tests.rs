use super::*;

#[test]
fn checkout_source_preserves_forge_identity_and_rejects_invalid_request_numbers() {
    let source: ChangeRequestCheckoutSource = serde_json::from_value(serde_json::json!({
        "kind":"change_request","forge":"github","number":123,"projectPath":"owner/repo",
    }))
    .unwrap();
    let input = source.into_intent();
    assert_eq!(input.number, 123);
    assert_eq!(input.forge.as_deref(), Some("github"));
    assert_eq!(input.project_path.as_deref(), Some("owner/repo"));
    for number in [
        serde_json::json!(0),
        serde_json::json!(-1),
        serde_json::json!(1.5),
        serde_json::json!(9_007_199_254_740_992_u64),
    ] {
        assert!(
            serde_json::from_value::<ChangeRequestCheckoutSource>(
                serde_json::json!({"kind":"change_request","number":number})
            )
            .is_err()
        );
    }
}
