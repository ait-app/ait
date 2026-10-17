use super::*;

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
