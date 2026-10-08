use super::*;

#[test]
fn summary_selection_preserves_provider_model_and_reasoning_wire_fields() {
    let selection = SummarySelection {
        provider: "codex".into(),
        model: Some("chosen".into()),
        thinking_option_id: Some("low".into()),
    };
    let value = serde_json::to_value(&selection).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"provider":"codex","model":"chosen","thinkingOptionId":"low"})
    );
    assert_eq!(
        serde_json::from_value::<SummarySelection>(value).unwrap(),
        selection
    );
    assert_eq!(SummarySelection::default().model, None);
}
