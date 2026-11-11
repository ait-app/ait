//! Native configuration facts: full snapshots are observations only; explicit change events
//! persist the accepted selection; pre-acknowledgement staleness is fenced by supersession.

use super::*;
use crate::local::omp::session::open;
use crate::local::omp::tests::{fixture, next};
use crate::ports::agent_session::AgentSession;
use crate::protocol::agent_config::NullableSetting;
use serde_json::json;

#[tokio::test]
async fn config_update_persists_the_accepted_selection() {
    let (_root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();

    session
        .consume(&json!({
            "type": "config_update",
            "model": {"provider": "other", "id": "model"},
            "thinkingLevel": "high",
        }))
        .unwrap();

    // An explicit native change is an accepted selection update: it is persisted so it
    // survives reopen, not merely reported as a runtime fact.
    assert_eq!(
        session.config.model.as_deref(),
        Some(r#"["other","model"]"#)
    );
    assert_eq!(session.config.thinking_option_id.as_deref(), Some("high"));
    let selection = next(&mut session).await.unwrap();
    assert_eq!(
        selection,
        AgentTurnEvent::ModelSelectionUpdated {
            model: NullableSetting::Set(r#"["other","model"]"#.to_owned()),
            thinking_option_id: NullableSetting::Set("high".to_owned()),
        }
    );
    assert!(matches!(
        next(&mut session).await.unwrap(),
        AgentTurnEvent::RuntimeInfo(_)
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn unchanged_observation_does_not_publish_or_rewrite_configuration() {
    let (_root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    let state = session.state.clone();
    let config = session.config.clone();

    session
        .consume(&json!({
            "type": "config_update",
            "model": state["model"],
            "thinkingLevel": state["thinkingLevel"],
        }))
        .unwrap();

    assert_eq!(session.config, config);
    assert!(session.stream.events.is_empty());
    session.close().await.unwrap();
}

#[tokio::test]
async fn malformed_native_model_facts_fail_closed_without_committing() {
    let (_root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    let config = session.config.clone();

    let result = session.consume(&json!({
        "type": "config_update",
        "model": {"provider": "", "id": "model"},
    }));

    assert!(result.is_err());
    assert_eq!(session.config, config);
    assert!(session.stream.events.is_empty());
    session.close().await.unwrap();
}

#[tokio::test]
async fn full_snapshot_observes_runtime_without_creating_an_override() {
    let (_root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    let before = session.config.clone();
    assert_eq!(before.model, None, "the session starts inherited");

    // A full get_state snapshot reflecting a different native model is an observation only:
    // it refreshes runtime facts but never converts inherited None into an explicit override.
    session
        .consume(&json!({
            "type": "config_update",
            "sessionId": session.state["sessionId"],
            "model": {"provider": "other", "id": "model"},
            "thinkingLevel": "high",
        }))
        .unwrap();

    assert_eq!(
        session.config, before,
        "no override is created from a snapshot"
    );
    let info = match next(&mut session).await.unwrap() {
        AgentTurnEvent::RuntimeInfo(info) => info,
        other => panic!("expected RuntimeInfo, got {other:?}"),
    };
    assert_eq!(info.model.as_deref(), Some(r#"["other","model"]"#));
    assert!(session.stream.events.is_empty());
    session.close().await.unwrap();
}

#[tokio::test]
async fn full_snapshot_does_not_revert_a_stored_override() {
    let (_root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    let mut config = session.config.clone();
    config.model = Some(r#"["other","model"]"#.to_owned());
    session.configure(&config).await.unwrap();
    session.stream.events.clear();

    // A stale full snapshot (the previous model) must not revert the stored override.
    session
        .consume(&json!({
            "type": "config_update",
            "sessionId": session.state["sessionId"],
            "model": {"provider": "test", "id": "model"},
        }))
        .unwrap();

    assert_eq!(
        session.config.model.as_deref(),
        Some(r#"["other","model"]"#),
        "the stored override is preserved against a stale snapshot"
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn model_changed_refresh_snapshot_keeps_the_override() {
    let (_root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    let mut config = session.config.clone();
    config.model = Some(r#"["other","model"]"#.to_owned());
    session.configure(&config).await.unwrap();
    session.stream.events.clear();

    session.consume(&json!({"type": "model_changed"})).unwrap();
    assert!(session.settings_refresh.pending());

    session.poll_state_check().unwrap();
    let check = session.state_check.take().unwrap();
    assert!(check.refresh_settings);
    session.state_check = Some(check);
    session
        .consume(&json!({
            "type": "response",
            "command": "get_state",
            "id": session.state_check.as_ref().unwrap().id,
            "success": true,
            "data": {
                "sessionId": session.state["sessionId"],
                "model": {"provider": "other", "id": "model"},
                "thinkingLevel": session.state["thinkingLevel"],
                "isSettled": true,
            },
        }))
        .unwrap();

    assert_eq!(
        session.config.model.as_deref(),
        Some(r#"["other","model"]"#),
        "the correlated full snapshot keeps the stored override"
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn acknowledged_selection_supersedes_pre_ack_buffered_changes() {
    let (_root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();

    // Ait applies a model change. The fixture emits model_changed after the set_model ack;
    // that event is buffered while the readback is in flight and must be superseded by the
    // confirmed snapshot, so it cannot later revert the acknowledged selection.
    let mut config = session.config.clone();
    config.model = Some(r#"["other","model"]"#.to_owned());
    session.configure(&config).await.unwrap();

    assert_eq!(
        session.config.model.as_deref(),
        Some(r#"["other","model"]"#)
    );
    // Drain everything the apply produced; no stale selection event may remain queued.
    while session.stream.events.pop_front().is_some() {}
    session.poll_turn().unwrap();
    assert!(
        session.stream.events.is_empty(),
        "pre-ack selection events are superseded by the confirmed readback"
    );
    assert_eq!(
        session.config.model.as_deref(),
        Some(r#"["other","model"]"#),
        "the acknowledged selection is not reverted"
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn post_ack_fresh_native_change_still_persists() {
    let (_root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();

    // Ait acknowledges a model change; the ack fence supersedes only pre-ack buffered events.
    let mut config = session.config.clone();
    config.model = Some(r#"["other","model"]"#.to_owned());
    session.configure(&config).await.unwrap();
    while session.stream.events.pop_front().is_some() {}

    // A fresh explicit native change arriving AFTER the ack remains authoritative: it is
    // accepted (last accepted writer wins) and persisted even over the prior user override.
    session
        .consume(&json!({
            "type": "config_update",
            "model": {"provider": "test", "id": "model"},
            "thinkingLevel": "high",
        }))
        .unwrap();

    assert_eq!(
        session.config.model.as_deref(),
        Some(r#"["test","model"]"#),
        "the post-ack fresh change updates the stored selection"
    );
    assert_eq!(session.config.thinking_option_id.as_deref(), Some("high"));
    assert!(matches!(
        session.stream.events.front(),
        Some(AgentTurnEvent::ModelSelectionUpdated { .. })
    ));
    session.close().await.unwrap();
}
