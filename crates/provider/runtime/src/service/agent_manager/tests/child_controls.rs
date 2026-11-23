use super::*;
use domain::agent_runtime::observation::{ObservationHealth, ObservedAgent, ObservedStatus};
use provider_core::ports::controls::{ChildControlFailure, ChildControlOutcome, ChildGuideOutcome};

fn child() -> PersistedAgentRuntimeRecord {
    let mut record = stored_record();
    record.id = "child-1".to_owned();
    record.persistence = None;
    record.observation = Some(ObservedAgent {
        revision: 3,
        root_agent_id: "agent-1".to_owned(),
        root_session_id: "native-1".to_owned(),
        parent_agent_id: "agent-1".to_owned(),
        native_id: "native-1/job".to_owned(),
        parent_native_id: "native-1".to_owned(),
        persistence: None,
        ready: true,
        status: ObservedStatus::Running,
        health: ObservationHealth::Current,
        observed_at: "2026-10-30T01:00:00Z".to_owned(),
        description: None,
        subtitle: None,
    });
    record
}

#[tokio::test]
async fn child_control_rejects_wrong_owner_stale_or_invalid_requests_before_native_write() {
    let (mut manager, registry, client) = make_manager();
    registry.upsert(&stored_record()).unwrap();
    registry.upsert(&child()).unwrap();
    assert!(matches!(
        manager.guide_child("agent-1", "child-1", "go", None),
        Err(AgentManagerError::Busy)
    ));
    registry.remove("agent-1").unwrap();
    manager
        .create("agent-1", &spec(), AgentRegistration::default())
        .await
        .unwrap();
    client.0.lock().unwrap().child_control_supported = ChildControlSupport::Supported;
    for mutate in 0..7 {
        let mut invalid = child();
        let observed = invalid.observation.as_mut().unwrap();
        match mutate {
            0 => observed.parent_agent_id = "other".to_owned(),
            1 => observed.root_agent_id = "other".to_owned(),
            2 => observed.root_session_id = "old".to_owned(),
            3 => observed.parent_native_id = "old".to_owned(),
            4 => observed.health = ObservationHealth::Stale,
            5 => observed.ready = false,
            6 => observed.status = ObservedStatus::Completed,
            _ => unreachable!(),
        }
        registry.upsert(&invalid).unwrap();
        assert!(
            matches!(
                manager.guide_child("agent-1", "child-1", "go", None),
                Err(AgentManagerError::Busy)
            ),
            "case {mutate}"
        );
    }
    registry.upsert(&child()).unwrap();
    let parent = registry.get("agent-1").unwrap().unwrap();
    for mutate in 0..2 {
        let mut invalid = parent.clone();
        if mutate == 0 {
            invalid.internal = true;
        } else {
            invalid.archived_at = Some("2026-11-01T00:00:00Z".to_owned());
        }
        registry.upsert(&invalid).unwrap();
        assert!(matches!(
            manager.stop_child("agent-1", "child-1", None),
            Err(AgentManagerError::Busy)
        ));
    }
    registry.upsert(&parent).unwrap();
    assert!(matches!(
        manager.guide_child("agent-1", "child-1", "go", Some(4)),
        Err(AgentManagerError::Busy)
    ));
    assert!(matches!(
        manager.guide_child("agent-1", "child-1", "  ", None),
        Err(AgentManagerError::InvalidRequest)
    ));
    assert!(matches!(
        manager.guide_child("agent-1", "child-1", &"x".repeat(65_537), None),
        Err(AgentManagerError::InvalidRequest)
    ));
    assert!(client.0.lock().unwrap().child_control_requests.is_empty());
}

#[tokio::test]
async fn child_control_dispatches_verified_native_id_and_correlates_later_outcomes() {
    let (mut manager, registry, client) = make_manager();
    manager
        .create("agent-1", &spec(), AgentRegistration::default())
        .await
        .unwrap();
    registry.upsert(&child()).unwrap();
    assert!(matches!(
        manager.guide_child("agent-1", "child-1", "go", None),
        Err(AgentManagerError::SessionRejected)
    ));
    client.0.lock().unwrap().child_control_supported = ChildControlSupport::Supported;
    let receipt = manager
        .guide_child("agent-1", "child-1", "  guide\n", Some(3))
        .unwrap();
    assert_eq!(receipt.receipt.token, "receipt-1");
    assert_eq!(
        client.0.lock().unwrap().child_control_requests,
        [("native-1/job".to_owned(), Some("  guide\n".to_owned()))]
    );
    assert!(manager.take_completed_child_controls().is_empty());
    client
        .0
        .lock()
        .unwrap()
        .events
        .push_back(AgentTurnEvent::ChildControl {
            token: "receipt-1".to_owned(),
            outcome: ChildControlOutcome::Guide(Ok(ChildGuideOutcome::Accepted)),
        });
    manager.poll().await.unwrap();
    assert_eq!(
        manager.take_completed_child_controls(),
        [(
            "agent-1".to_owned(),
            "receipt-1".to_owned(),
            ChildControlOutcome::Guide(Ok(ChildGuideOutcome::Accepted))
        )]
    );
    let receipt = manager.stop_child("agent-1", "child-1", Some(3)).unwrap();
    client
        .0
        .lock()
        .unwrap()
        .events
        .push_back(AgentTurnEvent::ChildControl {
            token: receipt.receipt.token,
            outcome: ChildControlOutcome::Stop(Err(ChildControlFailure::Uncertain)),
        });
    manager.poll().await.unwrap();
    assert!(matches!(
        manager.take_completed_child_controls().as_slice(),
        [(
            _,
            _,
            ChildControlOutcome::Stop(Err(ChildControlFailure::Uncertain))
        )]
    ));
}

#[tokio::test]
async fn child_control_pending_budget_is_bounded_per_parent() {
    let (mut manager, registry, client) = make_manager();
    manager
        .create("agent-1", &spec(), AgentRegistration::default())
        .await
        .unwrap();
    registry.upsert(&child()).unwrap();
    client.0.lock().unwrap().child_control_supported = ChildControlSupport::Supported;
    for _ in 0..8 {
        manager.stop_child("agent-1", "child-1", None).unwrap();
    }
    assert!(matches!(
        manager.stop_child("agent-1", "child-1", None),
        Err(AgentManagerError::Busy)
    ));
    assert_eq!(client.0.lock().unwrap().child_control_requests.len(), 8);
}
