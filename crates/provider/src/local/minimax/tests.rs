use super::*;
use crate::ports::agent_session::AgentTurnEvent;
use crate::protocol::prompt::AgentPrompt;

mod execution;
mod live;

fn fixture() -> (tempfile::TempDir, MiniMaxClient, AgentSessionSpec) {
    let root = tempfile::tempdir().unwrap();
    let client = MiniMaxClient::new(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/local/minimax/tests/fixtures/acp.py"),
    )
    .with_image_directory(root.path().join("images"));
    let spec = AgentSessionSpec {
        provider: PROVIDER.into(),
        cwd: root.path().to_str().unwrap().into(),
        config: StoredAgentConfig::default(),
    };
    (root, client, spec)
}

async fn finish(session: &mut dyn AgentSession) -> Vec<AgentTurnEvent> {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut events = Vec::new();
        loop {
            if let Some(event) = session.poll_turn().unwrap() {
                let terminal = matches!(
                    event,
                    AgentTurnEvent::Completed(_)
                        | AgentTurnEvent::Failed
                        | AgentTurnEvent::Cancelled
                );
                events.push(event);
                if terminal {
                    return events;
                }
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn discovers_native_models_modes_and_confirmed_configuration() {
    let (_root, client, mut spec) = fixture();
    assert!(client.is_available().await.unwrap());
    let details = client.discover(&spec.cwd).await.unwrap();
    assert_eq!(details.models.len(), 2);
    assert_eq!(
        details.models[0]["thinkingOptions"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(
        details.models[1]["thinkingOptions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(details.modes[0]["id"], "default");
    spec.config.model = Some("minimax/pro".into());
    spec.config.mode_id = Some("plan".into());
    spec.config.feature_values = Some(std::collections::BTreeMap::from([(
        "permissionMode".into(),
        json!("default"),
    )]));
    let mut session = client.create_session(&spec).await.unwrap();
    let info = session.runtime_info().await.unwrap();
    assert_eq!(info.model.as_deref(), Some("minimax/pro"));
    assert_eq!(info.mode_id.as_deref(), Some("plan"));
    assert_eq!(
        session.control_settings(&spec.config).unwrap()["features"][0]["value"],
        "default"
    );
    assert_eq!(
        session.control_settings(&spec.config).unwrap()["availableModes"][1]["id"],
        "plan"
    );
    session.close().await.unwrap();

    // Native effort choices disappear when selecting a model without effort control.
    spec.config.thinking_option_id = Some("low".into());
    assert!(matches!(
        client.create_session(&spec).await,
        Err(AgentSessionError::Rejected)
    ));
    spec.config.model = Some("minimax/flash".into());
    let mut session = client.create_session(&spec).await.unwrap();
    assert_eq!(
        session
            .runtime_info()
            .await
            .unwrap()
            .thinking_option_id
            .as_deref(),
        Some("low")
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn image_input_has_durable_sources_and_rejects_live_launch_changes() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let prompt = AgentPrompt {
        images: vec![crate::protocol::prompt::PromptImage {
            data: "aW1hZ2U=".into(),
            mime_type: "image/png".into(),
        }],
        client_message_id: Some("client-message".into()),
        ..Default::default()
    };
    session.start_input(&prompt, &spec.config).await.unwrap();
    let events = finish(session.as_mut()).await;
    assert!(events.iter().any(|event| matches!(
        event,
        AgentTurnEvent::Timeline(entry)
            if entry.item["type"] == "user_message"
                && entry.item["clientMessageId"] == "client-message"
                && entry.item["imageSources"].as_array().is_some_and(|sources| sources.len() == 1)
    )));
    let changed = StoredAgentConfig {
        system_prompt: Some("different".into()),
        ..Default::default()
    };
    assert!(session.validate_config_update(&changed).is_err());
    assert!(session.start_turn("hello", &changed).await.is_err());
    session.close().await.unwrap();
}

#[tokio::test]
async fn streams_text_thoughts_and_resumes_the_native_session() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_input(&AgentPrompt::text("remember"), &spec.config)
        .await
        .unwrap();
    let events = finish(session.as_mut()).await;
    assert!(events.iter().any(|event| matches!(
        event,
        AgentTurnEvent::Progress { entry, .. } if entry.item["type"] == "reasoning"
    )));
    assert_eq!(
        events.last(),
        Some(&AgentTurnEvent::Completed(Some("remembered".into())))
    );
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let mut session = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    session.start_turn("recall", &spec.config).await.unwrap();
    assert_eq!(
        finish(session.as_mut()).await.last(),
        Some(&AgentTurnEvent::Completed(Some("remembered".into())))
    );
    session.close().await.unwrap();
    assert!(
        client
            .resume_session(&handle, &spec, AgentResumePurpose::History)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn permission_callbacks_require_explicit_choices_and_resolve_once() {
    for (behavior, text) in [("allow", "approved"), ("deny", "denied")] {
        let (_root, client, spec) = fixture();
        let mut session = client.create_session(&spec).await.unwrap();
        session.start_turn("approve", &spec.config).await.unwrap();
        let permission = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(AgentTurnEvent::PermissionRequested(permission)) =
                    session.poll_turn().unwrap()
                {
                    break permission;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let id = permission["id"].as_str().unwrap();
        assert!(
            session
                .respond_permission(id, &json!({"behavior":"invalid"}))
                .await
                .is_err()
        );
        session
            .respond_permission(id, &json!({"behavior":behavior}))
            .await
            .unwrap();
        assert!(
            session
                .respond_permission(id, &json!({"behavior":behavior}))
                .await
                .is_err()
        );
        assert_eq!(
            finish(session.as_mut()).await.last(),
            Some(&AgentTurnEvent::Completed(Some(text.into())))
        );
        assert!(session.pending_permissions().is_empty());
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn cancellation_waits_for_acknowledgement_and_allows_the_next_turn() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    assert!(session.start_turn("busy", &spec.config).await.is_err());
    assert!(session.cancel_turn("other").await.is_err());
    session.cancel_turn(&turn).await.unwrap();
    assert_eq!(
        finish(session.as_mut()).await.last(),
        Some(&AgentTurnEvent::Cancelled)
    );
    session.start_turn("hello", &spec.config).await.unwrap();
    assert_eq!(
        finish(session.as_mut()).await.last(),
        Some(&AgentTurnEvent::Completed(Some(
            "Hello from MiniMax".into()
        )))
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn failed_authentication_and_native_errors_do_not_become_success() {
    let (root, client, spec) = fixture();
    std::fs::write(root.path().join("scenario"), "auth").unwrap();
    assert!(client.create_session(&spec).await.is_err());
    std::fs::write(root.path().join("scenario"), "normal").unwrap();
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("fail", &spec.config).await.unwrap();
    assert_eq!(
        finish(session.as_mut()).await.last(),
        Some(&AgentTurnEvent::Failed)
    );
    let message = session.failure_message().unwrap();
    assert!(message.contains("rate limit"));
    assert!(!message.contains("secret12345678"));
    session.close().await.unwrap();
}

#[tokio::test]
async fn rejects_invalid_models_handles_directories_and_unsupported_settings() {
    let (_root, client, mut spec) = fixture();
    spec.config.model = Some("unknown".into());
    assert!(client.create_session(&spec).await.is_err());
    spec.config = StoredAgentConfig::default();
    let mut session = client.create_session(&spec).await.unwrap();
    let mut handle = session.persistence().unwrap();
    session.close().await.unwrap();
    handle.provider = "mmx".into();
    assert!(
        client
            .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
            .await
            .is_err()
    );
    spec.cwd = "relative".into();
    assert!(client.create_session(&spec).await.is_err());
    for config in [
        StoredAgentConfig {
            model: Some("\n".into()),
            ..Default::default()
        },
        StoredAgentConfig {
            mode_id: Some(String::new()),
            ..Default::default()
        },
        StoredAgentConfig {
            tool_policy: Some(json!({"allow":true})),
            ..Default::default()
        },
    ] {
        assert!(client.validate_config(&config).is_err());
    }
}

#[tokio::test]
async fn missing_cancel_acknowledgement_is_bounded_and_closes_the_native_writer() {
    let (root, mut client, spec) = fixture();
    std::fs::write(root.path().join("scenario"), "ignore-cancel").unwrap();
    client.deadline = Duration::from_secs(1);
    let mut session = client.create_session(&spec).await.unwrap();
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    assert!(session.cancel_turn(&turn).await.is_err());
    assert!(session.start_turn("retry", &spec.config).await.is_err());
    session.close().await.unwrap();
}

#[tokio::test]
async fn rejects_foreign_session_updates_instead_of_settling_the_turn() {
    let (root, client, spec) = fixture();
    std::fs::write(root.path().join("scenario"), "wrong-session").unwrap();
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("hello", &spec.config).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match session.poll_turn() {
                Err(_) => break,
                Ok(Some(AgentTurnEvent::Completed(_))) => panic!("foreign turn completed"),
                _ => tokio::time::sleep(Duration::from_millis(5)).await,
            }
        }
    })
    .await
    .unwrap();
    session.close().await.unwrap();
}
