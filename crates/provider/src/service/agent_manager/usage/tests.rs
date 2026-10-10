use super::*;

#[test]
fn quota_cache_reuses_bounded_facts_and_force_refresh_bypasses_them() {
    let cache = UsageCache::default();
    let value = json!({"providerId":"codex","windows":[]});
    cache.put("codex", &value);
    assert_eq!(cache.get("codex", false), Some(value));
    assert!(cache.get("codex", true).is_none());
    assert!(cache.get("other", false).is_none());
    cache.0.lock().unwrap().get_mut("codex").unwrap().0 = Instant::now()
        .checked_sub(Duration::from_secs(301))
        .unwrap();
    assert!(cache.get("codex", false).is_none());
    assert_eq!(
        failure("claude", AgentSessionError::Unavailable)["problem"]["kind"],
        "no_quota"
    );
    assert_eq!(
        failure("codex", AgentSessionError::Failed)["status"],
        "error"
    );
    for index in 0..65 {
        cache.put(&format!("provider-{index}"), &json!({"providerId":index}));
    }
    assert!(cache.0.lock().unwrap().len() <= 64);
    assert!(cache.get("provider-64", false).is_some());
}

#[tokio::test]
async fn selecting_one_host_provider_does_not_return_another_cached_account() {
    let mut manager = AgentManager::new(Box::new(super::super::tests::MemoryRegistry::default()));
    manager
        .register_client(Box::new(crate::local::codex::CodexClient::new(
            "unused-codex".into(),
        )))
        .unwrap();
    manager
        .register_client(Box::new(crate::local::claude::ClaudeClient::new(
            "unused-claude".into(),
        )))
        .unwrap();
    for id in ["codex", "claude"] {
        manager.usage_cache.put(id, &json!({"providerId":id,"accountLabel":format!("{id}@example.test"),"status":"available","planLabel":null,"windows":[]}));
    }
    let report = manager.usage(None, Some("codex"), false).await.unwrap();
    assert_eq!(report["providers"].as_array().unwrap().len(), 1);
    assert_eq!(report["providers"][0]["accountLabel"], "codex@example.test");
    assert_eq!(
        manager.usage(None, Some("unknown"), false).await,
        Err(ErrorCode::UnsupportedCapability)
    );
}
