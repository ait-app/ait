use super::*;
use crate::protocol::resume::Overrides;

#[tokio::test]
async fn failed_registration_and_failed_cleanup_retain_the_session_until_retry_succeeds() {
    let (mut manager, registry, client) = make_manager();
    let record = stored_record();
    registry.0.lock().unwrap().fail_upsert = true;
    client.0.lock().unwrap().fail_close = true;
    assert_eq!(
        manager.restore_new(record.clone()).await,
        Err(AgentManagerError::Session)
    );
    assert_eq!(
        registry.list().unwrap(),
        [] as [PersistedAgentRuntimeRecord; 0]
    );
    assert_eq!(client.0.lock().unwrap().close_calls, 1);
    assert_eq!(
        manager.resume(&record.id).await,
        Err(AgentManagerError::Session)
    );
    assert_eq!(client.0.lock().unwrap().resume_specs.len(), 1);
    client.0.lock().unwrap().fail_close = false;
    manager.close(&record.id).await.unwrap();
    assert!(manager.live.is_empty());
    assert_eq!(client.0.lock().unwrap().close_calls, 2);
    registry.0.lock().unwrap().fail_upsert = false;
    assert_eq!(
        manager.restore_new(record.clone()).await.unwrap().id,
        record.id
    );
}

#[tokio::test]
async fn resume_rejects_a_handle_from_another_provider_without_native_io() {
    let (mut manager, registry, client) = make_manager();
    let mut record = stored_record();
    record.persistence.as_mut().unwrap().provider = "claude".to_owned();
    registry.upsert(&record).unwrap();
    assert_eq!(
        manager.resume(&record.id).await,
        Err(AgentManagerError::InvalidRequest)
    );
    assert_eq!(registry.get(&record.id).unwrap(), Some(record));
    assert_eq!(
        client.0.lock().unwrap().resume_specs,
        [] as [AgentSessionSpec; 0]
    );
}

#[tokio::test]
async fn unknown_native_restore_registers_only_after_open_and_preserves_failure_cleanup() {
    let (mut manager, registry, client) = make_manager();
    let record = stored_record();
    registry.0.lock().unwrap().fail_upsert = true;
    assert_eq!(
        manager.restore_new(record.clone()).await,
        Err(AgentManagerError::Registry)
    );
    assert_eq!(
        registry.list().unwrap(),
        [] as [PersistedAgentRuntimeRecord; 0]
    );
    assert!(manager.live.is_empty());
    assert_eq!(client.0.lock().unwrap().close_calls, 1);
    registry.0.lock().unwrap().fail_upsert = false;
    let restored = manager.restore_new(record.clone()).await.unwrap();
    assert_eq!(restored.id, record.id);
    assert_eq!(registry.get(&record.id).unwrap(), Some(restored));
    assert!(matches!(
        manager.restore_new(record).await,
        Err(AgentManagerError::AlreadyExists(_))
    ));
    assert_eq!(client.0.lock().unwrap().resume_specs.len(), 2);
}

#[test]
fn explicit_title_overrides_clear_automatic_title_provenance() {
    let mut record = stored_record();
    record.title_origin = Some(domain::agent_runtime::TitleOrigin::Generated);
    let cleared: Overrides = serde_json::from_value(json!({"title":null})).unwrap();
    cleared.apply(&mut record);
    assert!(record.title.is_none());
    assert!(record.title_origin.is_none());
    let renamed: Overrides = serde_json::from_value(json!({"title":"Manual"})).unwrap();
    renamed.apply(&mut record);
    assert_eq!(record.title.as_deref(), Some("Manual"));
    assert!(record.title_origin.is_none());
}

#[tokio::test]
async fn explicit_restore_merges_only_supplied_fields_and_preserves_concurrent_metadata() {
    let (mut manager, registry, client) = make_manager();
    let mut record = stored_record();
    record.archived_at = Some("2026-09-21T10:00:00Z".to_owned());
    record.config.as_mut().unwrap().thinking_option_id = Some("high".to_owned());
    registry.upsert(&record).unwrap();
    client.0.lock().unwrap().during_resume = Some(registry.clone());
    let overrides: Overrides = serde_json::from_value(json!({
        "cwd":"/tmp/changed","model":"replacement","systemPrompt":"new prompt",
        "featureValues":{"fast_mode":true},"providerOptions":{"codex":{"serviceTier":"fast"}},
        "toolPolicy":{"preapproved":[]},"mcpServers":{},"modeId":"read-only"
    }))
    .unwrap();
    let restored = manager.restore(&record.id, &overrides).await.unwrap();
    let native = client.0.lock().unwrap().resume_specs[0].clone();
    assert_eq!(native.cwd, "/tmp/changed");
    assert_eq!(native.config, restored.config.clone().unwrap());
    assert_eq!(native.config.thinking_option_id.as_deref(), Some("high"));
    assert_eq!(native.config.model.as_deref(), Some("replacement"));
    assert_eq!(native.config.system_prompt.as_deref(), Some("new prompt"));
    assert_eq!(
        native.config.feature_values,
        overrides.config.feature_values
    );
    assert_eq!(
        native.config.provider_options,
        overrides.config.provider_options
    );
    assert_eq!(native.config.tool_policy, overrides.config.tool_policy);
    assert_eq!(native.config.mcp_servers, overrides.config.mcp_servers);
    assert!(restored.archived_at.is_none());
    assert_eq!(restored.title.as_deref(), Some("Changed during resume"));
    assert!(restored.requires_attention);
    assert_eq!(restored.workspace_id, record.workspace_id);
    assert_eq!(
        client.0.lock().unwrap().resume_purposes,
        [AgentResumePurpose::Interactive]
    );
    assert_eq!(
        manager
            .restore(&record.id, &Overrides::default())
            .await
            .unwrap(),
        restored
    );
    assert_eq!(client.0.lock().unwrap().resume_specs.len(), 1);
}

#[tokio::test]
async fn failed_restore_registration_closes_the_new_session_without_committing_overrides() {
    let (mut manager, registry, client) = make_manager();
    let mut record = stored_record();
    record.archived_at = Some("2026-09-21T10:00:00Z".to_owned());
    registry.upsert(&record).unwrap();
    registry.0.lock().unwrap().fail_update = true;
    let overrides: Overrides =
        serde_json::from_value(json!({"title":null,"model":"replacement"})).unwrap();
    assert_eq!(
        manager.restore(&record.id, &overrides).await,
        Err(AgentManagerError::Registry)
    );
    assert_eq!(registry.get(&record.id).unwrap(), Some(record));
    assert_eq!(client.0.lock().unwrap().close_calls, 1);
    assert!(manager.live.is_empty());
}

#[tokio::test]
async fn restore_rejects_missing_handles_and_mismatched_providers_before_native_io() {
    let (mut manager, registry, client) = make_manager();
    assert!(matches!(
        manager.restore("missing", &Overrides::default()).await,
        Err(AgentManagerError::NotFound(_))
    ));
    let mut record = stored_record();
    record.persistence = None;
    registry.upsert(&record).unwrap();
    assert!(matches!(
        manager.restore(&record.id, &Overrides::default()).await,
        Err(AgentManagerError::MissingPersistence(_))
    ));
    record.persistence = stored_record().persistence;
    record.persistence.as_mut().unwrap().provider = "claude".to_owned();
    registry.upsert(&record).unwrap();
    assert_eq!(
        manager.restore(&record.id, &Overrides::default()).await,
        Err(AgentManagerError::InvalidRequest)
    );
    assert_eq!(
        client.0.lock().unwrap().resume_specs,
        [] as [AgentSessionSpec; 0]
    );
}
