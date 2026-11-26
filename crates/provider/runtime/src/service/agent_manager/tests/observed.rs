use super::*;
use crate::service::agent_manager::observed::{register_children, scope};
use crate::storage::timeline::Timeline;
use domain::agent_runtime::observation::{ObservationHealth, ObservedStatus};
use provider_core::ports::controls::NativeSubagent;
use provider_core::ports::native_history::{SessionDescriptor, SessionHistory};
use provider_core::protocol::timeline::NativeItem;

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
    let entry = NativeItem {
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
                        &provider_core::protocol::prompt::AgentPrompt::text("write"),
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

#[tokio::test]
async fn opening_live_child_does_not_complete_items_ahead_of_queued_stream_events() {
    use provider_core::ports::controls::SubagentEvent;

    let (mut manager, registry, client) = make_manager();
    manager
        .create("agent-1", &spec(), AgentRegistration::default())
        .await
        .expect("create");
    manager
        .send("agent-1", "start")
        .await
        .expect("start parent");
    manager.poll().await.expect("poll parent");
    let timeline = Timeline::memory().expect("timeline");
    manager = manager.with_timeline(timeline.clone());
    let root = registry.get("agent-1").expect("registry").expect("root");
    let native = child("native-child", "native-1", "running");
    let ids = register_children(&registry, &timeline, &root, std::slice::from_ref(&native))
        .expect("register child");
    let id = &ids["native-child"];
    let entry = NativeItem {
        key: "native:child-turn:answer".to_owned(),
        turn_id: Some("child-turn".to_owned()),
        timestamp: "2026-10-30T01:00:00Z".to_owned(),
        item: serde_json::json!({"type":"assistant_message","text":"answer"}),
    };
    {
        let mut state = client.0.lock().expect("state");
        state.subagents = vec![native];
        // The provider has completed the message, but its live notifications are still queued.
        state.inspected_history = Some(child_history(&entry));
        state
            .events
            .push_back(AgentTurnEvent::Subagent(SubagentEvent::Progress {
                id: "native-child".to_owned(),
                observation: "late-delta".to_owned(),
                entry: NativeItem {
                    item: serde_json::json!({"type":"assistant_message","text":"ans"}),
                    ..entry.clone()
                },
            }));
        state
            .events
            .push_back(AgentTurnEvent::Subagent(SubagentEvent::Timeline {
                id: "native-child".to_owned(),
                entry: entry.clone(),
            }));
    }
    manager.load_timeline(id).await.expect("open live child");
    manager.poll().await.expect("drain queued child events");
    assert_eq!(manager.active_turn("agent-1"), Some("native-turn"));
    assert_eq!(client.0.lock().expect("state").close_calls, 0);
    assert_eq!(client.0.lock().expect("state").inspect_calls, 0);
    assert!(
        !manager.history_loaded(id),
        "deferred reads must remain retryable"
    );
    let source = registry
        .get(id)
        .expect("registry")
        .expect("child")
        .observation
        .expect("source");
    let rows = timeline.read(&scope(&source)).expect("child timeline").1;
    assert_eq!(
        rows.iter()
            .filter_map(|row| row.entry.item["text"].as_str())
            .collect::<String>(),
        "answer"
    );

    // Once the live owner is gone, opening the child must still recover authoritative history.
    manager.live.remove("agent-1");
    client
        .0
        .lock()
        .expect("state")
        .inspected_history
        .as_mut()
        .expect("history")
        .active = false;
    manager
        .load_timeline(id)
        .await
        .expect("load settled history");
    assert_eq!(client.0.lock().expect("state").inspect_calls, 1);
    assert!(manager.history_loaded(id));
    assert_eq!(
        registry
            .get(id)
            .expect("registry")
            .expect("child")
            .observation
            .expect("source")
            .native_id,
        "native-child"
    );
}

fn child_history(entry: &NativeItem) -> SessionHistory {
    SessionHistory {
        resume_metadata: BTreeMap::new(),
        parent_id: Some("native-1".to_owned()),
        descriptor: SessionDescriptor {
            provider_id: "codex".to_owned(),
            provider_label: "Codex".to_owned(),
            provider_handle_id: "native-child".to_owned(),
            cwd: "/tmp".to_owned(),
            title: None,
            first_prompt_preview: None,
            last_prompt_preview: None,
            last_activity_at: entry.timestamp.clone(),
        },
        created_at: entry.timestamp.clone(),
        config: spec().config,
        active: true,
        entries: vec![entry.clone()],
    }
}
