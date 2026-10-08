use super::*;
use domain::workspace::lifecycle::WorkspaceCreation;
use model::workspace::lifecycle::WorkspaceDirectory;

#[test]
fn directory_port_shares_registration_restoration_and_observer_resources() {
    let changes = model::changes::Changes::default();
    let mut receiver = changes.subscribe();
    let directory = directory().with_changes(changes);
    let port: Arc<dyn WorkspaceDirectory> = Arc::new(directory.clone());
    let created = port
        .create_workspace(WorkspaceCreation {
            path: "/tmp/alpha",
            title: Some("Created through port".to_owned()),
            project_id: Some("prj_a"),
            workspace_id: Some("wks_port".to_owned()),
            expects_initial_agent: true,
            timestamp: "created",
        })
        .unwrap();
    assert_eq!(created.title.as_deref(), Some("Created through port"));
    assert!(directory.list_workspaces().unwrap().contains(&created));
    directory.archive_workspace("wks_a", "archived").unwrap();
    directory.archive_workspace("wks_port", "archived").unwrap();
    let restored = port.open_workspace("/tmp/alpha", "restored").unwrap();
    assert_eq!(restored.workspace_id, "wks_a");
    assert!(restored.archived_at.is_none());
    receiver.borrow_and_update();
    port.changes().unwrap().notify();
    assert!(receiver.has_changed().unwrap());
    receiver.borrow_and_update();
    assert!(port.shared_worktrees().is_none());
}

#[test]
fn directory_port_preserves_safe_business_failures() {
    let directory = directory();
    let port: &dyn WorkspaceDirectory = &directory;
    assert_eq!(
        port.open_workspace("/missing", "now").unwrap_err().message,
        "directory not found"
    );
    let error = port
        .create_workspace(WorkspaceCreation {
            path: "/tmp/alpha",
            title: None,
            project_id: Some("missing"),
            workspace_id: None,
            expects_initial_agent: false,
            timestamp: "now",
        })
        .unwrap_err();
    assert_eq!(error.message, "unknown project");
    assert!(port.changes().is_none());
}

#[test]
fn project_registration_port_reuses_shared_records_and_preserves_safe_failures() {
    use model::workspace::lifecycle::ProjectRegistration;

    let directory = directory();
    let port: Arc<dyn ProjectRegistration> = Arc::new(directory.clone());
    let project = port.register_project("/tmp/alpha", "registered").unwrap();
    assert_eq!(project.project_id, "prj_a");
    assert!(directory.list_projects().unwrap().contains(&project));
    let reused = port.register_project("/tmp/alpha", "again").unwrap();
    assert_eq!(reused.project_id, project.project_id);
    assert_eq!(reused.created_at, project.created_at);
    assert_eq!(
        port.register_project("/missing", "now")
            .unwrap_err()
            .message,
        "directory not found"
    );
}
