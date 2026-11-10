use super::*;
use crate::protocol::agent_config::NullableSetting;
use crate::protocol::agent_execution::ActiveTurnBehavior;
use crate::protocol::prompt::AgentPrompt;
use crate::service::agent_manager::delivery::Admission;
use crate::storage::timeline::{Timeline, inbox::Receipt};

#[tokio::test]
async fn a_poll_failure_does_not_drop_the_written_commands_durable_completion() {
    let root = tempfile::tempdir().unwrap();
    let timeline = Timeline::open(&root.path().join("timeline.sqlite")).unwrap();
    let (manager, registry, client) = make_manager();
    let mut manager = manager.with_timeline(timeline.clone());
    manager
        .create("agent-1", &spec(), AgentRegistration::default())
        .await
        .unwrap();
    {
        let mut state = client.0.lock().unwrap();
        state.command_registry = Some(registry.clone());
    }
    let prompt = AgentPrompt {
        client_message_id: Some("command-id".into()),
        ..AgentPrompt::text("query")
    };
    let admitted = manager
        .deliver("agent-1", &prompt, ActiveTurnBehavior::Interrupt)
        .await
        .unwrap();
    assert!(manager.poll().await.is_err());
    let Admission::Pending(completion) = admitted else {
        panic!("command receipt must remain owned");
    };
    completion.await.unwrap();
    assert_eq!(
        timeline
            .input_receipt("agent-1", "command-id", &prompt, "answer")
            .unwrap(),
        Some(Receipt::Accepted)
    );
    assert!(matches!(
        manager
            .deliver("agent-1", &prompt, ActiveTurnBehavior::Interrupt)
            .await
            .unwrap(),
        Admission::Accepted
    ));
    assert_eq!(client.0.lock().unwrap().commands_written, 1);
    registry.0.lock().unwrap().fail_update = false;
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn partial_native_configuration_keeps_unmentioned_overrides_and_inheritance() {
    let (mut manager, registry, client) = make_manager();
    let mut specification = spec();
    specification.config.model = None;
    manager
        .create("agent-1", &specification, AgentRegistration::default())
        .await
        .unwrap();
    for (model, thinking, expected_model, expected_thinking) in [
        (
            NullableSetting::Unchanged,
            NullableSetting::Set("high".into()),
            None,
            Some("high"),
        ),
        (
            NullableSetting::Set("native-model".into()),
            NullableSetting::Unchanged,
            Some("native-model"),
            Some("high"),
        ),
        (
            NullableSetting::Unchanged,
            NullableSetting::Clear,
            Some("native-model"),
            None,
        ),
    ] {
        let event = AgentTurnEvent::ModelSelectionUpdated {
            model,
            thinking_option_id: thinking,
        };
        let wire = serde_json::to_value(&event).unwrap();
        let event = serde_json::from_value(wire).unwrap();
        client.0.lock().unwrap().events.push_back(event);
        manager.poll().await.unwrap();
        let config = registry.get("agent-1").unwrap().unwrap().config.unwrap();
        assert_eq!(config.model.as_deref(), expected_model);
        assert_eq!(config.thinking_option_id.as_deref(), expected_thinking);
    }
    manager.close_all().await.unwrap();
}
