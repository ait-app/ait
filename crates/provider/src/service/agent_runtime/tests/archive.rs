use super::*;

#[test]
fn observed_children_keep_verified_parentage_and_only_archive_confirmed_results() {
    use domain::agent_runtime::observation::{ObservationHealth, ObservedAgent, ObservedStatus};
    let (service, registry) = service();
    let mut child = agent("observed", "wks-one", "Task", false);
    child
        .labels
        .insert(PARENT_AGENT_ID_LABEL.to_owned(), "agent-a".to_owned());
    child.labels.insert(
        format!("{OPEN_AGENT_TAB_LABEL_PREFIX}client"),
        "true".to_owned(),
    );
    child.last_status = AgentRuntimeStatus::Running;
    child.observation = Some(ObservedAgent {
        revision: 0,
        root_agent_id: "agent-a".to_owned(),
        root_session_id: "root".to_owned(),
        parent_agent_id: "agent-a".to_owned(),
        native_id: "child".to_owned(),
        parent_native_id: "root".to_owned(),
        persistence: None,
        ready: true,
        status: ObservedStatus::Running,
        health: ObservationHealth::Current,
        observed_at: child.updated_at.clone(),
        description: None,
        subtitle: None,
    });
    registry.upsert(&child).expect("observed child");
    assert!(service.detach("observed", "now").is_err());
    assert!(service.archive("observed", "now").is_err());
    service
        .update(
            "observed",
            None,
            Some(&BTreeMap::from([(
                PARENT_AGENT_ID_LABEL.to_owned(),
                "foreign".to_owned(),
            )])),
            "now",
        )
        .expect("metadata update");
    assert_eq!(
        registry
            .get("observed")
            .expect("registry")
            .expect("child")
            .labels[PARENT_AGENT_ID_LABEL],
        "agent-a"
    );
    service.archive("agent-a", "now").expect("archive parent");
    let archived = registry.get("observed").expect("registry").expect("child");
    assert!(archived.archived_at.is_some());
    assert_eq!(
        archived
            .observation
            .as_ref()
            .expect("source")
            .parent_agent_id,
        "agent-a"
    );
    assert_eq!(
        archived.observation.as_ref().expect("source").health,
        ObservationHealth::Unavailable
    );
    assert_eq!(
        archived.observation.as_ref().expect("source").status,
        ObservedStatus::Running
    );
}

#[test]
fn workspace_batch_archives_all_owned_agents_and_detaches_cross_workspace_children() {
    let (service, registry) = service();
    let mut other = agent("other", "wks-two", "Other", false);
    other
        .labels
        .insert(PARENT_AGENT_ID_LABEL.into(), "agent-a".into());
    registry.upsert(&other).unwrap();
    registry
        .upsert(&agent("internal", "wks-one", "Internal", true))
        .unwrap();
    assert_eq!(
        service
            .archive_workspaces(&["wks-one".into(), "wks-one".into()], "archived")
            .unwrap(),
        ["agent-a", "agent-b", "internal"]
    );
    let other = registry.get("other").unwrap().unwrap();
    assert!(other.archived_at.is_none());
    assert!(!other.labels.contains_key(PARENT_AGENT_ID_LABEL));
    assert_eq!(
        registry
            .get("internal")
            .unwrap()
            .unwrap()
            .archived_at
            .as_deref(),
        Some("archived")
    );
    assert_eq!(
        service
            .archive_workspaces(&["missing".into()], "later")
            .unwrap(),
        [] as [String; 0]
    );
    service
        .archive_workspaces(&["wks-one".into(), "wks-two".into()], "later")
        .unwrap();
    assert!(
        registry
            .list()
            .unwrap()
            .iter()
            .all(|record| record.archived_at.is_some())
    );
}
