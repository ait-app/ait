use super::*;
use crate::ports::controls::NativeSubagent;
use crate::service::agent_manager::observed::{register_children, scope};
use crate::storage::timeline::Timeline;
use domain::agent_runtime::observation::{ObservationHealth, ObservedStatus};

fn child(id: &str, parent: &str, status: &str) -> NativeSubagent {
    NativeSubagent {
        id: id.to_owned(),
        parent_id: parent.to_owned(),
        cwd: "/tmp".to_owned(),
        persistence: None,
        descriptor: serde_json::json!({"id":id,"provider":"codex","title":"Explore","description":format!("Inspect {id}"),"status":status,"createdAt":"2026-10-30T00:00:00Z","updatedAt":"2026-10-30T01:00:00Z"}),
    }
}

#[test]
fn managed_native_identity_is_reused_without_forwarding_a_second_stream() {
    let (_, registry, _) = make_manager();
    let root = stored_record();
    registry.upsert(&root).expect("root");
    let mut managed = stored_record();
    managed.id = "managed-child".to_owned();
    managed.persistence.as_mut().expect("handle").session_id = "native-child".to_owned();
    registry.upsert(&managed).expect("managed child");
    let timeline = Timeline::memory().expect("timeline");
    let native = &root.persistence.as_ref().expect("root handle").session_id;
    let ids = register_children(
        &registry,
        &timeline,
        &root,
        &[child("native-child", native, "running")],
    )
    .expect("reuse");
    assert_eq!(ids["native-child"], managed.id);
    let saved = registry
        .get(&managed.id)
        .expect("registry")
        .expect("managed");
    assert!(saved.observation.is_none());
    assert_eq!(saved.persistence, managed.persistence);
    assert_eq!(saved.labels["paseo.parent-agent-id"], root.id);
    assert_eq!(registry.list().expect("records").len(), 2);
    let (outbound, mut receiver) = model::outbound::Outbound::new();
    let subscription = timeline.events().subscribe(
        "managed".to_owned(),
        std::collections::BTreeSet::from([managed.id.clone()]),
        outbound,
    );
    subscription.activate().expect("subscribe");
    let entry = crate::protocol::timeline::NativeItem {
        key: "native:item".to_owned(),
        turn_id: None,
        timestamp: "2026-10-31T00:00:00Z".to_owned(),
        item: serde_json::json!({"type":"assistant_message","text":"answer"}),
    };
    timeline
        .append(
            &format!("subagent:{}:native-child", root.id),
            "codex",
            std::slice::from_ref(&entry),
        )
        .expect("legacy child history");
    assert!(receiver.try_recv().is_err());
    timeline
        .append(&managed.id, "codex", &[entry])
        .expect("managed writer history");
    assert!(receiver.try_recv().is_ok());
}

#[test]
fn native_title_updates_preserve_manual_renames_and_advance_only_changed_observations() {
    let (_, registry, _) = make_manager();
    let root = stored_record();
    registry.upsert(&root).expect("root");
    let timeline = Timeline::memory().expect("timeline");
    let native = &root.persistence.as_ref().expect("handle").session_id;
    let mut children = [child("child", native, "running")];
    let ids = register_children(&registry, &timeline, &root, &children).expect("register");
    let id = &ids["child"];
    let initial = registry.get(id).expect("registry").expect("child");
    assert_eq!(initial.title.as_deref(), Some("Inspect child"));
    register_children(&registry, &timeline, &root, &children).expect("retry");
    assert_eq!(registry.get(id).expect("registry"), Some(initial.clone()));

    children[0].descriptor["description"] = serde_json::json!("Inspect persistence");
    register_children(&registry, &timeline, &root, &children).expect("native rename");
    let renamed = registry.get(id).expect("registry").expect("child");
    assert_eq!(renamed.title.as_deref(), Some("Inspect persistence"));
    assert!(
        renamed.observation.as_ref().expect("source").revision
            > initial.observation.as_ref().expect("source").revision
    );
    registry
        .update(id, &|current| {
            let mut next = current.clone();
            next.title = Some("My investigation".to_owned());
            next.title_origin = None;
            next
        })
        .expect("manual rename");
    children[0].descriptor["description"] = serde_json::json!("Inspect execution");
    register_children(&registry, &timeline, &root, &children).expect("later observation");
    assert_eq!(
        registry
            .get(id)
            .expect("registry")
            .expect("child")
            .title
            .as_deref(),
        Some("My investigation")
    );
}

#[test]
fn malformed_cached_children_do_not_prevent_other_agents_from_starting() {
    let (manager, registry, _) = make_manager();
    let root = stored_record();
    registry.upsert(&root).expect("root");
    let timeline = Timeline::memory().expect("timeline");
    let mut invalid = child(
        "invalid",
        &root.persistence.as_ref().expect("handle").session_id,
        "running",
    );
    invalid.descriptor["updatedAt"] = serde_json::json!("invalid timestamp");
    timeline
        .store_subagent(&root.id, &invalid)
        .expect("legacy descriptor");
    let manager = manager.with_timeline(timeline.clone());
    manager
        .recover_observed()
        .expect("skip invalid legacy identity");
    assert_eq!(
        timeline.subagents(&root.id).expect("preserved history"),
        [invalid]
    );
    assert_eq!(registry.list().expect("records").len(), 1);

    let native = &root.persistence.as_ref().expect("handle").session_id;
    timeline
        .store_subagent(&root.id, &child(native, native, "running"))
        .expect("legacy ancestry cycle");
    manager.recover_observed().expect("skip invalid ancestry");
}

#[test]
fn registration_is_stable_nested_read_only_and_preserves_source_cursors() {
    let (_, registry, _) = make_manager();
    let root = stored_record();
    registry.upsert(&root).expect("root");
    let native = &root.persistence.as_ref().expect("handle").session_id;
    let children = [
        child("grandchild", "child", "completed"),
        child("child", native, "running"),
    ];
    let timeline = Timeline::memory().expect("timeline");
    let ids = register_children(&registry, &timeline, &root, &children).expect("register tree");
    let initial = registry
        .get(&ids["child"])
        .expect("registry")
        .expect("child");
    assert!(initial.persistence.is_none());
    assert_eq!(
        initial
            .observation
            .as_ref()
            .expect("source")
            .parent_agent_id,
        root.id
    );
    assert!(initial.observation.as_ref().expect("source").ready);
    let grandchild = registry
        .get(&ids["grandchild"])
        .expect("registry")
        .expect("grandchild");
    assert_eq!(
        grandchild
            .observation
            .as_ref()
            .expect("source")
            .parent_agent_id,
        ids["child"]
    );
    let scope = scope(initial.observation.as_ref().expect("source"));
    let epoch = timeline.generation(&scope).expect("generation");
    assert_eq!(
        register_children(&registry, &timeline, &root, &children).expect("retry"),
        ids
    );
    assert_eq!(timeline.generation(&scope).expect("generation"), epoch);
    assert_eq!(registry.list().expect("records").len(), 3);
}

#[test]
fn unfinished_registration_recovers_without_allocating_another_identity() {
    let (manager, registry, _) = make_manager();
    let root = stored_record();
    registry.upsert(&root).expect("root");
    let children = [child(
        "child",
        &root.persistence.as_ref().expect("handle").session_id,
        "running",
    )];
    let timeline = Timeline::memory().expect("timeline");
    registry.0.lock().expect("registry").fail_update = true;
    assert!(register_children(&registry, &timeline, &root, &children).is_err());
    let pending = registry
        .list()
        .expect("records")
        .into_iter()
        .find(|record| record.observation.is_some())
        .expect("pending");
    assert!(!pending.observation.as_ref().expect("source").ready);
    registry.0.lock().expect("registry").fail_update = false;
    let manager = manager.with_timeline(timeline);
    manager.recover_observed().expect("recover");
    let recovered = registry.get(&pending.id).expect("registry").expect("child");
    let source = recovered.observation.expect("source");
    assert!(source.ready);
    assert_eq!(source.health, ObservationHealth::Stale);
    assert_eq!(source.status, ObservedStatus::Running);
    assert_eq!(recovered.last_status, AgentRuntimeStatus::Closed);
    assert_eq!(registry.list().expect("records").len(), 2);

    let stale = registry.get(&pending.id).expect("registry");
    registry.0.lock().expect("registry").fail_update = true;
    manager
        .recover_observed()
        .expect("already stale observations are not rewritten");
    assert_eq!(registry.get(&pending.id).expect("registry"), stale);
}

#[test]
fn recovery_keeps_archived_root_children_unavailable_without_rewriting_them() {
    let (manager, registry, _) = make_manager();
    let mut root = stored_record();
    root.archived_at = Some("2026-10-31T00:00:00Z".to_owned());
    registry.upsert(&root).expect("root");
    let timeline = Timeline::memory().expect("timeline");
    let native = &root.persistence.as_ref().expect("handle").session_id;
    let ids = register_children(
        &registry,
        &timeline,
        &root,
        &[child("child", native, "running")],
    )
    .expect("register");
    let observed = registry.get(&ids["child"]).expect("registry");
    assert_eq!(
        observed
            .as_ref()
            .and_then(|record| record.observation.as_ref())
            .map(|source| source.health),
        Some(ObservationHealth::Unavailable)
    );

    registry.0.lock().expect("registry").fail_update = true;
    manager
        .with_timeline(timeline)
        .recover_observed()
        .expect("unavailable observations are not rewritten");
    assert_eq!(registry.get(&ids["child"]).expect("registry"), observed);
}

#[test]
fn independent_execution_is_rejected_before_provider_or_input_side_effects() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime")
        .block_on(async {
            let (manager, registry, client) = make_manager();
            let root = stored_record();
            registry.upsert(&root).expect("root");
            let timeline = Timeline::memory().expect("timeline");
            let children = [child(
                "child",
                &root.persistence.as_ref().expect("handle").session_id,
                "running",
            )];
            let ids = register_children(&registry, &timeline, &root, &children).expect("register");
            let id = &ids["child"];
            let mut manager = manager.with_timeline(timeline.clone());
            let original = registry.get(id).expect("registry").expect("child");
            assert_eq!(manager.resume(id).await, Err(AgentManagerError::Busy));
            assert_eq!(manager.cancel(id).await, Err(AgentManagerError::Busy));
            assert!(
                manager
                    .deliver(
                        id,
                        &crate::protocol::prompt::AgentPrompt::text("write"),
                        crate::protocol::agent_execution::ActiveTurnBehavior::Interrupt
                    )
                    .await
                    .is_err()
            );
            assert_eq!(
                registry.get(id).expect("registry").expect("child"),
                original
            );
            assert!(!timeline.has_queued_input(id).expect("queue"));
            assert_eq!(client.0.lock().expect("client").create_calls, 0);
            assert!(client.0.lock().expect("client").resume_purposes.is_empty());
        });
}

#[test]
fn unavailable_observations_keep_outcomes_and_stale_lists_cannot_reverse_completion() {
    let (_, registry, _) = make_manager();
    let root = stored_record();
    registry.upsert(&root).expect("root");
    let timeline = Timeline::memory().expect("timeline");
    let native = &root.persistence.as_ref().expect("handle").session_id;
    let mut finished = child("child", native, "completed");
    finished.descriptor["updatedAt"] = serde_json::json!("2026-10-30T02:00:00Z");
    let ids = register_children(&registry, &timeline, &root, &[finished]).expect("completed");
    register_children(
        &registry,
        &timeline,
        &root,
        &[child("child", native, "running")],
    )
    .expect("old snapshot");
    assert_eq!(
        registry
            .get(&ids["child"])
            .expect("registry")
            .expect("child")
            .observation
            .expect("source")
            .status,
        ObservedStatus::Completed
    );
    let mut uncertain = child("unknown", native, "completed");
    uncertain.descriptor["outcomeKnown"] = serde_json::json!(false);
    let ids = register_children(&registry, &timeline, &root, &[uncertain]).expect("unknown state");
    assert_eq!(
        registry
            .get(&ids["unknown"])
            .expect("registry")
            .expect("child")
            .observation
            .expect("source")
            .status,
        ObservedStatus::Unknown
    );
}

fn observed_child(id: &str, parent: &str, status: ObservedStatus) -> PersistedAgentRuntimeRecord {
    let mut record = stored_record();
    record.id = id.to_owned();
    record.persistence = None;
    record.observation = Some(domain::agent_runtime::observation::ObservedAgent {
        revision: 1,
        root_agent_id: parent.to_owned(),
        root_session_id: "native-1".to_owned(),
        parent_agent_id: parent.to_owned(),
        native_id: format!("native-{id}"),
        parent_native_id: "native-1".to_owned(),
        persistence: None,
        ready: true,
        status,
        health: ObservationHealth::Current,
        observed_at: "2026-10-30T01:00:00.000Z".to_owned(),
        description: Some("Inspect".to_owned()),
        subtitle: None,
    });
    record
}

#[tokio::test]
async fn child_controls_reject_a_non_live_parent_without_a_native_write() {
    let (mut manager, registry, _) = make_manager();
    registry.upsert(&stored_record()).expect("parent");
    registry
        .upsert(&observed_child(
            "child-1",
            "agent-1",
            ObservedStatus::Running,
        ))
        .expect("child");
    // The parent record exists but no live writer owns it in this process.
    assert!(manager.live_snapshot("agent-1").is_none());
    assert_eq!(
        manager
            .guide_child("agent-1", "child-1", "go", None)
            .await
            .err(),
        Some(AgentManagerError::Busy)
    );
    assert_eq!(
        manager.stop_child("agent-1", "child-1", None).await.err(),
        Some(AgentManagerError::Busy)
    );
}

#[tokio::test]
async fn child_controls_reject_wrong_parent_and_stale_observation() {
    let (mut manager, registry, _) = make_manager();
    manager
        .create("agent-1", &spec(), AgentRegistration::default())
        .await
        .expect("live parent");
    registry
        .upsert(&observed_child(
            "child-1",
            "another-parent",
            ObservedStatus::Running,
        ))
        .expect("child");
    // Wrong parent: the child's observed parent chain does not match the claimed parent.
    assert_eq!(
        manager
            .guide_child("agent-1", "child-1", "go", None)
            .await
            .err(),
        Some(AgentManagerError::Busy)
    );
    // Stale observation health blocks control even with a matching parent.
    let mut stale = observed_child("child-1", "agent-1", ObservedStatus::Running);
    stale.observation.as_mut().expect("source").health = ObservationHealth::Stale;
    registry.upsert(&stale).expect("stale child");
    assert_eq!(
        manager.stop_child("agent-1", "child-1", None).await.err(),
        Some(AgentManagerError::Busy)
    );
    // A mismatched expected revision is fenced before any native write.
    let current = observed_child("child-1", "agent-1", ObservedStatus::Running);
    registry.upsert(&current).expect("current child");
    assert!(
        manager
            .guide_child("agent-1", "child-1", "go", Some(99))
            .await
            .is_err_and(|error| error == AgentManagerError::Busy)
    );
}

#[tokio::test]
async fn guide_requires_a_running_child_and_blank_guidance_is_invalid() {
    let (mut manager, registry, _) = make_manager();
    manager
        .create("agent-1", &spec(), AgentRegistration::default())
        .await
        .expect("live parent");
    registry
        .upsert(&observed_child(
            "child-1",
            "agent-1",
            ObservedStatus::Completed,
        ))
        .expect("child");
    assert_eq!(
        manager
            .guide_child("agent-1", "child-1", "go", None)
            .await
            .err(),
        Some(AgentManagerError::Busy)
    );
    assert_eq!(
        manager
            .guide_child("agent-1", "child-1", "   ", None)
            .await
            .err(),
        Some(AgentManagerError::InvalidRequest)
    );
}

#[tokio::test]
async fn child_controls_reject_providers_without_native_child_support() {
    let (mut manager, registry, _) = make_manager();
    manager
        .create("agent-1", &spec(), AgentRegistration::default())
        .await
        .expect("live parent");
    registry
        .upsert(&observed_child(
            "child-1",
            "agent-1",
            ObservedStatus::Running,
        ))
        .expect("child");
    // The default session port rejects child controls; no second writer is created.
    assert_eq!(
        manager
            .guide_child("agent-1", "child-1", "go", None)
            .await
            .err(),
        Some(AgentManagerError::SessionRejected)
    );
    assert_eq!(
        manager.stop_child("agent-1", "child-1", None).await.err(),
        Some(AgentManagerError::SessionRejected)
    );
    assert!(manager.live_snapshot("child-1").is_none());
}

#[tokio::test]
async fn child_controls_reject_observations_from_a_replaced_native_owner() {
    let (mut manager, registry, _) = make_manager();
    manager
        .create("agent-1", &spec(), AgentRegistration::default())
        .await
        .expect("live parent");
    let mut child = observed_child("child-1", "agent-1", ObservedStatus::Running);
    child.observation.as_mut().expect("source").root_session_id = "previous-native".to_owned();
    registry.upsert(&child).expect("previous owner's child");
    assert_eq!(
        manager
            .guide_child("agent-1", "child-1", "go", None)
            .await
            .err(),
        Some(AgentManagerError::Busy)
    );
    assert_eq!(
        manager.stop_child("agent-1", "child-1", None).await.err(),
        Some(AgentManagerError::Busy)
    );

    let mut child = observed_child("child-1", "agent-1", ObservedStatus::Running);
    child.provider = "omp".to_owned();
    registry.upsert(&child).expect("another provider's child");
    assert_eq!(
        manager.stop_child("agent-1", "child-1", None).await.err(),
        Some(AgentManagerError::Busy)
    );
    assert!(manager.live_snapshot("child-1").is_none());
}

#[tokio::test]
async fn child_control_dispatch_returns_before_the_native_answer_arrives() {
    use crate::ports::controls::{
        ChildControlFailure, ChildControlOutcome, ChildControlReceipt, ChildGuideOutcome,
    };
    let (mut manager, registry, client) = make_manager();
    manager
        .create("agent-1", &spec(), AgentRegistration::default())
        .await
        .expect("live parent");
    registry
        .upsert(&observed_child(
            "child-1",
            "agent-1",
            ObservedStatus::Running,
        ))
        .expect("child");
    client.0.lock().unwrap().child_control_result =
        Some(ChildControlOutcome::Guide(Ok(ChildGuideOutcome::Accepted)));
    // The write is acknowledged immediately; the typed answer arrives on a later drain.
    let dispatched = manager
        .guide_child("agent-1", "child-1", "go", None)
        .await
        .expect("dispatched");
    assert_eq!(
        dispatched.receipt,
        ChildControlReceipt {
            token: "receipt-1".to_owned()
        }
    );
    assert_eq!(manager.take_completed_child_controls(), []);
    manager.poll().await.expect("drain");
    assert_eq!(
        manager.take_completed_child_controls(),
        [(
            "receipt-1".to_owned(),
            ChildControlOutcome::Guide(Ok(ChildGuideOutcome::Accepted))
        )]
    );

    // A refusal reaches the reply as a typed failure instead of an error on the write path.
    client.0.lock().unwrap().child_control_result = Some(ChildControlOutcome::Stop(Err(
        ChildControlFailure::Rejected,
    )));
    let dispatched = manager
        .stop_child("agent-1", "child-1", None)
        .await
        .expect("dispatched");
    assert_eq!(dispatched.receipt.token, "receipt-1");
    manager.poll().await.expect("drain");
    assert_eq!(
        manager.take_completed_child_controls(),
        [(
            "receipt-1".to_owned(),
            ChildControlOutcome::Stop(Err(ChildControlFailure::Rejected))
        )]
    );
}
