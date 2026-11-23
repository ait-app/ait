use super::*;
use domain::agent_runtime::registry::AgentRuntimeRegistry;
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
use provider::service::agent_manager::AgentManager;
use provider::testing::Manager;
use provider_core::protocol::agent_config::ConfigPatch;

#[tokio::test]
async fn configuration_during_an_active_turn_applies_before_the_next_prompt() {
    let (root, client, spec) = fixture();
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    let mut manager = Manager::new(AgentManager::new(Box::new(registry.clone())));
    manager.register(Box::new(client)).unwrap();
    manager.create("agent", &spec).await.unwrap();
    let mut frames = crate::transport::subscribe_frames();
    manager.send("agent", "wait-config").await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while count(&calls_after(root.path(), 0), "prompt") == 0 {
            manager.poll().await.unwrap();
            if count(&calls_after(root.path(), 0), "prompt") == 0 {
                let _ = frames.recv().await;
            }
        }
    })
    .await
    .unwrap();
    assert!(manager.active_turn("agent").is_some());

    let before = calls_after(root.path(), 0).len();
    let patch: ConfigPatch = serde_json::from_value(json!({
        "modelId": "[\"test\",\"any-effort\"]",
        "thinkingOptionId": "high",
        "featureValues": {"fastMode": true}
    }))
    .unwrap();
    manager.configure("agent", &patch).await.unwrap();
    manager.poll().await.unwrap();
    assert!(manager.active_turn("agent").is_some());
    let config = registry.get("agent").unwrap().unwrap().config.unwrap();
    assert_eq!(config.model.as_deref(), Some("[\"test\",\"any-effort\"]"));
    assert_eq!(config.thinking_option_id.as_deref(), Some("high"));
    assert_eq!(config.feature_values.unwrap()["fastMode"], true);
    let during = calls_after(root.path(), before);
    for kind in ["set_model", "set_thinking_level", "set_fast_mode", "abort"] {
        assert_eq!(count(&during, kind), 0, "unexpected {kind}: {during:?}");
    }

    std::fs::write(root.path().join("agent/release-config"), "").unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while manager.active_turn("agent").is_some() {
            manager.poll().await.unwrap();
            if manager.active_turn("agent").is_some() {
                let _ = frames.recv().await;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(manager.last_message("agent"), Some("Hello world"));
    for kind in ["set_model", "set_thinking_level", "set_fast_mode", "abort"] {
        let calls = calls_after(root.path(), before);
        assert_eq!(count(&calls, kind), 0, "unexpected {kind}: {calls:?}");
    }
    let before_next = calls_after(root.path(), 0).len();
    manager.send("agent", "second").await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while manager.active_turn("agent").is_some() {
            manager.poll().await.unwrap();
            if manager.active_turn("agent").is_some() {
                let _ = frames.recv().await;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(manager.last_message("agent"), Some("Hello world"));
    let next_calls = calls_after(root.path(), before_next);
    let prompt = next_calls
        .iter()
        .position(|call| call["type"] == "prompt")
        .unwrap();
    for kind in ["set_model", "set_thinking_level", "set_fast_mode"] {
        let setter = next_calls
            .iter()
            .position(|call| call["type"] == kind)
            .unwrap();
        assert!(
            setter < prompt,
            "{kind} was sent after prompt: {next_calls:?}"
        );
    }
    let model = next_calls
        .iter()
        .find(|call| call["type"] == "set_model")
        .unwrap();
    assert_eq!(model["provider"], "test");
    assert_eq!(model["modelId"], "any-effort");
    let thinking = next_calls
        .iter()
        .find(|call| call["type"] == "set_thinking_level")
        .unwrap();
    assert_eq!(thinking["level"], "high");
    let fast = next_calls
        .iter()
        .find(|call| call["type"] == "set_fast_mode")
        .unwrap();
    assert_eq!(fast["enabled"], true);
    manager.close_all().await.unwrap();
}
