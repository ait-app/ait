use super::*;
use crate::protocol::agent_config::NullableSetting;

#[tokio::test]
async fn temporary_configuration_preserves_durable_turn_facts_on_success_rejection_and_retry() {
    for (rejected, storage_failure) in [(false, false), (true, false), (false, true)] {
        let (mut manager, registry, client) = make_manager();
        let mut before = stored_record();
        before.last_status = AgentRuntimeStatus::Idle;
        before.last_error = Some("Last turn failed".into());
        before.last_mode_id = Some("last-turn-mode".into());
        registry.upsert(&before).unwrap();
        {
            let mut state = client.0.lock().unwrap();
            state.config_result = Some(Ok(()));
            state.effective_config = before.config.clone();
            if rejected {
                state.config_result = Some(Err(AgentSessionError::Rejected));
            }
            if storage_failure {
                state.during_configuration = Some(registry.clone());
            }
        }
        let patch = ConfigPatch {
            thinking_option_id: NullableSetting::Set("high".into()),
            ..ConfigPatch::default()
        };

        let result = manager.configure("agent-1", &patch).await;

        assert_eq!(
            result,
            if rejected {
                Err(AgentManagerError::SessionRejected)
            } else if storage_failure {
                Err(AgentManagerError::Registry)
            } else {
                Ok(ConfigurationOutcome::Applied)
            }
        );
        let after = registry.get("agent-1").unwrap().unwrap();
        assert_eq!(after.last_status, before.last_status);
        assert_eq!(after.last_error, before.last_error);
        assert_eq!(after.last_mode_id, before.last_mode_id);
        assert_eq!(after.last_activity_at, before.last_activity_at);
        assert_eq!(after.last_user_message_at, before.last_user_message_at);
        registry.0.lock().unwrap().fail_update = false;
        manager.poll().await.unwrap();
        assert!(!manager.has_sessions());
        let after = registry.get("agent-1").unwrap().unwrap();
        assert_eq!(after.last_status, before.last_status);
        assert_eq!(after.last_error, before.last_error);
        assert_eq!(after.last_mode_id, before.last_mode_id);
        assert_eq!(
            after.config.unwrap().thinking_option_id.as_deref(),
            (!rejected).then_some("high")
        );
        assert_eq!(
            client.0.lock().unwrap().resume_purposes,
            [AgentResumePurpose::Configuration {
                replace_model: false,
                replace_thinking: true,
            }]
        );
    }
}

#[tokio::test]
async fn runtime_observations_preserve_registry_repairs_without_extra_default_handle_reads() {
    for native_controls in [false, true] {
        let (mut manager, registry, client) = make_manager();
        manager
            .create("agent-1", &spec(), AgentRegistration::default())
            .await
            .unwrap();
        registry
            .update("agent-1", &|current| {
                let mut next = current.clone();
                next.persistence.as_mut().unwrap().metadata = Some(BTreeMap::from([
                    ("sessionFile".into(), json!("/repaired/session.jsonl")),
                    ("transportGate".into(), json!(true)),
                ]));
                next
            })
            .unwrap();
        if native_controls {
            client.0.lock().unwrap().effective_config = Some(spec().config);
        }
        let before = registry.get("agent-1").unwrap().unwrap();
        let mut info = before.runtime_info.clone().unwrap();
        info.model = Some("observed-model".into());
        let agent = &manager.live["agent-1"];
        let reads = client.0.lock().unwrap().persistence_reads;

        assert_eq!(
            streaming::persist_info(&registry, "agent-1", agent, &info),
            Ok(None)
        );

        let after = registry.get("agent-1").unwrap().unwrap();
        assert_eq!(after.runtime_info, Some(info.clone()));
        assert_eq!(after.persistence, before.persistence);
        assert_eq!(
            client.0.lock().unwrap().persistence_reads - reads,
            usize::from(native_controls)
        );
        registry.0.lock().unwrap().fail_update = true;
        assert_eq!(
            streaming::persist_info(&registry, "agent-1", agent, &info),
            Err(ErrorCode::AgentIo)
        );
        assert_eq!(registry.get("agent-1").unwrap().unwrap(), after);
        registry.0.lock().unwrap().fail_update = false;
        manager.close_all().await.unwrap();
    }
}

#[test]
fn history_refresh_preserves_queued_controls_while_repairing_resume_facts() {
    let mut record = stored_record();
    let deferred = json!({"config":{"thinkingOptionId":null},"model":false,"thinking":true});
    record.persistence.as_mut().unwrap().metadata = Some(BTreeMap::from([
        ("deferredConfig".into(), deferred.clone()),
        ("sessionFile".into(), json!("/old/session.jsonl")),
    ]));
    let history = crate::ports::native_history::SessionHistory {
        resume_metadata: BTreeMap::from([
            ("sessionFile".into(), json!("/new/session.jsonl")),
            ("hasInput".into(), json!(true)),
        ]),
        parent_id: None,
        descriptor: crate::ports::native_history::SessionDescriptor {
            provider_id: record.provider.clone(),
            provider_label: "Fixture".into(),
            provider_handle_id: record.persistence.as_ref().unwrap().session_id.clone(),
            cwd: record.cwd.clone(),
            title: None,
            first_prompt_preview: None,
            last_prompt_preview: None,
            last_activity_at: record.updated_at.clone(),
        },
        created_at: record.created_at.clone(),
        config: StoredAgentConfig::default(),
        active: false,
        entries: Vec::new(),
    };

    native_sessions::apply_history(&mut record, &history);

    let metadata = record.persistence.unwrap().metadata.unwrap();
    assert_eq!(metadata["deferredConfig"], deferred);
    assert_eq!(metadata["sessionFile"], "/new/session.jsonl");
    assert_eq!(metadata["hasInput"], true);
}

#[tokio::test]
async fn host_deferred_configuration_preserves_native_state_without_reading_a_handle() {
    for active in [false, true] {
        let (mut manager, registry, client) = make_manager();
        manager
            .create("agent-1", &spec(), AgentRegistration::default())
            .await
            .unwrap();
        if active {
            manager.send("agent-1", "hello").await.unwrap();
        }
        registry
            .update("agent-1", &|current| {
                let mut next = current.clone();
                next.persistence.as_mut().unwrap().metadata =
                    Some(BTreeMap::from([("retained".into(), Value::Bool(true))]));
                next
            })
            .unwrap();
        let before = registry.get("agent-1").unwrap().unwrap();
        let reads = client.0.lock().unwrap().persistence_reads;
        let patch = ConfigPatch {
            model_id: NullableSetting::Set("next-model".into()),
            ..ConfigPatch::default()
        };

        assert_eq!(
            manager.configure("agent-1", &patch).await.unwrap(),
            ConfigurationOutcome::Deferred
        );
        let after = registry.get("agent-1").unwrap().unwrap();
        assert_eq!(
            after.config.as_ref().unwrap().model.as_deref(),
            Some("next-model")
        );
        assert_eq!(after.persistence, before.persistence);
        assert_eq!(after.runtime_info, before.runtime_info);
        assert_eq!(client.0.lock().unwrap().persistence_reads, reads);
        assert_eq!(manager.active_turn("agent-1").is_some(), active);

        registry.0.lock().unwrap().fail_update = true;
        assert_eq!(
            manager.configure("agent-1", &patch).await,
            Err(AgentManagerError::Registry)
        );
        assert_eq!(registry.get("agent-1").unwrap().unwrap(), after);
        assert_eq!(client.0.lock().unwrap().persistence_reads, reads);
        registry.0.lock().unwrap().fail_update = false;
        manager.close_all().await.unwrap();
    }
}

#[tokio::test]
async fn configuration_resume_does_not_reserve_interactive_capacity() {
    let (mut manager, _, _) = make_manager();
    manager
        .create("dormant", &spec(), AgentRegistration::default())
        .await
        .unwrap();
    manager.close("dormant").await.unwrap();
    for index in 0..32 {
        manager
            .create(
                &format!("live-{index}"),
                &spec(),
                AgentRegistration::default(),
            )
            .await
            .unwrap();
    }
    assert_eq!(manager.session_budget.available_permits(), 0);
    manager
        .resume_with_purpose(
            "dormant",
            false,
            AgentResumePurpose::Configuration {
                replace_model: true,
                replace_thinking: false,
            },
        )
        .await
        .unwrap();
    assert!(manager.live["dormant"].permit.is_none());
    assert_eq!(manager.session_budget.available_permits(), 0);
    manager.close("dormant").await.unwrap();
    assert_eq!(manager.session_budget.available_permits(), 0);
    assert_eq!(
        manager
            .create("overflow", &spec(), AgentRegistration::default())
            .await,
        Err(AgentManagerError::Busy)
    );
    manager.close_all().await.unwrap();
}
