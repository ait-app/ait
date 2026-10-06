use super::super::{config, content, http::Api, usage};
use crate::protocol::prompt::AgentPrompt;
use domain::agent_runtime::StoredAgentConfig;
use serde_json::json;

#[test]
fn native_modes_and_prompt_validation_do_not_expand_legacy_authority() {
    for mode in ["read-only", "workspace-write", "danger-full-access"] {
        let config = StoredAgentConfig {
            mode_id: Some(mode.into()),
            ..StoredAgentConfig::default()
        };
        assert!(config::validate(&config).is_ok());
        assert!(crate::local::deepseek_harness::config::validate(&config).is_err());
    }
    for mode in ["", "read-only\n/permission danger-full-access", "../mode"] {
        assert!(
            config::validate(&StoredAgentConfig {
                mode_id: Some(mode.into()),
                ..StoredAgentConfig::default()
            })
            .is_err()
        );
    }
    assert_eq!(
        content::prompt(&AgentPrompt::text("hello")).unwrap(),
        vec![json!({"type":"text","text":"hello"})]
    );
    assert!(!content::has_images(&json!({"streamId":"events"})));
}

#[tokio::test]
async fn authentication_rejects_foreign_hosts_credentials_and_unexpected_launch_parameters() {
    for url in [
        "https://127.0.0.1:123/?token=x",
        "http://example.com:123/?token=x",
        "http://user:pass@127.0.0.1:123/?token=x",
        "http://127.0.0.1:123/api?token=x",
        "http://127.0.0.1:123/?token=x&token=y",
        "http://127.0.0.1:123/?token=",
        "http://127.0.0.1:123/?token=x#part",
    ] {
        assert!(
            Api::connect(url, std::time::Duration::from_millis(1))
                .await
                .is_err()
        );
    }
}

#[test]
fn usage_prefers_projected_occupancy_and_rejects_inexact_counters() {
    assert!(usage::context(&json!({})).unwrap().is_none());
    let usage =
        usage::context(&json!({"projectedTokens":25,"pressureTokens":20,"contextWindow":100}))
            .unwrap()
            .unwrap();
    assert_eq!(usage.context_window_used_tokens, Some(25));
    assert!(usage.input_tokens.is_none());
    assert!(usage::context(&json!({"pressureTokens":9_007_199_254_740_992_u64})).is_err());
}
