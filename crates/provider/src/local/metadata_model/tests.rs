use super::*;
use serde_json::json;

#[test]
fn chooses_available_small_model_and_low_supported_effort() {
    let models = vec![
        json!({"id":"large","isDefault":true}),
        json!({"id":"mini-disabled","isSelectable":false}),
        json!({"id":"mini","thinkingOptions":[{"id":"high"},{"id":"low"}]}),
    ];
    let selected = select("native", &models, &["mini"]).unwrap();
    assert_eq!(selected.provider, "native");
    assert_eq!(selected.model.as_deref(), Some("mini"));
    assert_eq!(selected.thinking_option_id.as_deref(), Some("low"));
    assert!(select("native", &models, &["haiku"]).is_none());
    assert!(select("native", &[json!({"label":"mini"})], &["mini"]).is_none());
}

#[test]
fn preference_order_precedes_catalog_order_and_labels_can_match() {
    let models = vec![
        json!({"id":"cheap"}),
        json!({"id":"opaque", "label":"Flash"}),
    ];
    let selected = select("native", &models, &["flash", "cheap"]).unwrap();
    assert_eq!(selected.model.as_deref(), Some("opaque"));
    assert_eq!(selected.thinking_option_id, None);
}
