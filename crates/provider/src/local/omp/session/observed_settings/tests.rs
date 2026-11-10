use super::*;

#[tokio::test]
async fn native_model_changes_publish_validated_configuration_before_runtime_facts() {
    use crate::ports::agent_session::AgentSession;
    let (_root, client, spec) = crate::local::omp::tests::fixture();
    let mut session = crate::local::omp::session::open(&client, &spec, None)
        .await
        .unwrap();
    session.stream.events.clear();
    session
        .observe_settings(
            &serde_json::json!({"model":{"provider":"other","id":"model"},"thinkingLevel":"high"}),
        )
        .unwrap();
    assert_eq!(
        session.config.model.as_deref(),
        Some("[\"other\",\"model\"]")
    );
    assert_eq!(session.config.thinking_option_id.as_deref(), Some("high"));
    assert!(matches!(
        session.stream.events.pop_front(),
        Some(AgentTurnEvent::ModelSelectionUpdated { .. })
    ));
    assert!(matches!(
        session.stream.events.pop_front(),
        Some(AgentTurnEvent::RuntimeInfo(_))
    ));
    assert!(
        session
            .observe_settings(&serde_json::json!({"model":{"id":"invalid"}}))
            .is_err()
    );
    assert_eq!(
        session.config.model.as_deref(),
        Some("[\"other\",\"model\"]")
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn native_plugin_model_events_request_a_nonblocking_snapshot_on_the_same_owner() {
    use crate::ports::agent_session::AgentSession;
    let (root, client, spec) = crate::local::omp::tests::fixture();
    let mut session = crate::local::omp::session::open(&client, &spec, None)
        .await
        .unwrap();
    session
        .transport
        .request(
            "set_model",
            serde_json::json!({"provider":"other","modelId":"model"}),
        )
        .await
        .unwrap();
    session
        .transport
        .request("set_thinking_level", serde_json::json!({"level":"high"}))
        .await
        .unwrap();
    session
        .consume(&serde_json::json!({"type":"model_changed"}))
        .unwrap();
    session
        .consume(&serde_json::json!({"type":"thinking_level_changed"}))
        .unwrap();
    assert_eq!(session.config.model, None);
    let events = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if let Some(AgentTurnEvent::ModelSelectionUpdated {
                model,
                thinking_option_id,
            }) = session.poll_turn().unwrap()
            {
                return (model, thinking_option_id);
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        events,
        (
            NullableSetting::Set("[\"other\",\"model\"]".to_owned()),
            NullableSetting::Set("high".to_owned())
        )
    );
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(!calls.contains("\"type\": \"prompt\""));
    session.close().await.unwrap();
}

#[tokio::test]
async fn partial_native_facts_publish_only_the_fields_that_were_carried() {
    use crate::ports::agent_session::AgentSession;
    let (_root, client, spec) = crate::local::omp::tests::fixture();
    let mut session = crate::local::omp::session::open(&client, &spec, None)
        .await
        .unwrap();
    session.config.model = None;
    session.stream.events.clear();
    session
        .observe_settings(&serde_json::json!({"thinkingLevel":"high"}))
        .unwrap();
    assert!(session.config.model.is_none());
    assert!(matches!(
        session.stream.events.pop_front(),
        Some(AgentTurnEvent::ModelSelectionUpdated {
            model: NullableSetting::Unchanged,
            thinking_option_id: NullableSetting::Set(_)
        })
    ));
    session.stream.events.clear();
    session.state["thinkingLevel"] = Value::Null;
    session
        .observe_settings(&serde_json::json!({"model":{"provider":"other","id":"model"}}))
        .unwrap();
    assert_eq!(session.config.thinking_option_id.as_deref(), Some("high"));
    assert!(matches!(
        session.stream.events.pop_front(),
        Some(AgentTurnEvent::ModelSelectionUpdated {
            model: NullableSetting::Set(_),
            thinking_option_id: NullableSetting::Unchanged
        })
    ));
    session.close().await.unwrap();
}
