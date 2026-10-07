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

#[test]
fn model_family_names_do_not_match_unrelated_small_model_tokens() {
    use crate::local::{deepseek_harness::DeepSeekHarnessClient, opencode::OpenCodeClient};
    use crate::ports::agent_session::AgentClient;
    for (client, models, expected) in [
        (
            Box::new(DeepSeekHarnessClient::new("dsh".into())) as Box<dyn AgentClient>,
            vec![
                json!({"id":"[\"google\",\"gemini-2.5-pro\"]","label":"Gemini 2.5 Pro"}),
                json!({"id":"[\"google\",\"gemini-2.5-flash\"]","label":"Gemini 2.5 Flash"}),
            ],
            "[\"google\",\"gemini-2.5-flash\"]",
        ),
        (
            Box::new(OpenCodeClient::new("opencode".into())) as Box<dyn AgentClient>,
            vec![
                json!({"id":"google/gemini-2.5-pro"}),
                json!({"id":"google/gemini-2.5-flash"}),
            ],
            "google/gemini-2.5-flash",
        ),
    ] {
        assert_eq!(
            client.summary_model(&models).unwrap().model.as_deref(),
            Some(expected)
        );
        assert!(client.summary_model(&models[..1]).is_none());
    }
    for name in ["gemini", "minimal", "minimax", "illuminate"] {
        assert!(!matches_preference(name, "mini"));
    }
    for name in [
        "GPT-5-MINI",
        "vendor/mini:latest",
        "Mini",
        "gpt-4.1-mini-2025",
    ] {
        assert!(matches_preference(name, "mini"));
    }
}
