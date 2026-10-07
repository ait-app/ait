use model::workspace::worktrees::{WorktreeAction, WorktreeCreation, WorktreeProvisioning};

use super::*;

#[test]
fn legacy_directory_placement_normalizes_the_new_branch_before_using_the_git_owner() {
    use model::workspace::worktrees::DirectoryGit;
    let managed = Managed::default();
    let adapter = WorkspaceWorktrees::new(Arc::new(Mutex::new(service(
        &Projects::default(),
        &Workspaces::default(),
        &managed,
    ))));
    adapter
        .prepare_directory(
            "/repo",
            &DirectoryGit::BranchOff {
                branch: "Feature Review".into(),
                base: Some("release".into()),
            },
        )
        .unwrap();
    adapter
        .prepare_directory(
            "/repo",
            &DirectoryGit::Checkout {
                branch: "main".into(),
            },
        )
        .unwrap();
    assert_eq!(
        managed.state.lock().unwrap().directory_inputs,
        vec![
            (
                "/repo".into(),
                DirectoryGit::BranchOff {
                    branch: "feature-review".into(),
                    base: Some("release".into())
                }
            ),
            (
                "/repo".into(),
                DirectoryGit::Checkout {
                    branch: "main".into()
                }
            )
        ]
    );
}

fn intent() -> WorktreeCreation {
    WorktreeCreation {
        cwd: Some("/repo/app".to_owned()),
        project_id: None,
        workspace_id: Some("wks_0123456789abcdef".to_owned()),
        title: Some(" Explicit title ".to_owned()),
        branch_name: Some("feature/review".to_owned()),
        base_branch: Some("develop".to_owned()),
        worktree_slug: Some("review".to_owned()),
        ref_name: None,
        action: WorktreeAction::BranchOff,
        checkout_source: None,
        first_agent_prompt: Some("Prompt fallback".to_owned()),
        expects_initial_agent: true,
    }
}

#[test]
fn shared_provisioning_archive_removes_only_the_registered_owned_worktree() {
    let projects = Projects::default();
    let workspaces = Workspaces::default();
    let managed = Managed::default();
    let adapter = WorkspaceWorktrees::new(Arc::new(Mutex::new(service(
        &projects,
        &workspaces,
        &managed,
    ))));
    let created = adapter.create(&intent(), "created").unwrap();
    adapter
        .archive(&created.workspace.workspace_id, "archived")
        .unwrap();
    assert_eq!(
        workspaces
            .get(&created.workspace.workspace_id)
            .unwrap()
            .unwrap()
            .archived_at
            .as_deref(),
        Some("archived")
    );
    assert_eq!(managed.state.lock().unwrap().removed.len(), 1);
    assert!(adapter.archive("unknown", "later").is_err());
    assert_eq!(managed.state.lock().unwrap().removed.len(), 1);
}

#[test]
fn unified_creation_preserves_reserved_id_title_branch_and_project_root() {
    let projects = Projects::default();
    projects.0.lock().unwrap().push(project("prj_selected"));
    let workspaces = Workspaces::default();
    let managed = Managed::default();
    let adapter = WorkspaceWorktrees::new(Arc::new(Mutex::new(service(
        &projects,
        &workspaces,
        &managed,
    ))));
    let mut input = intent();
    input.cwd = None;
    input.project_id = Some("prj_selected".to_owned());
    let created = adapter.create(&input, "2026-01-02T00:00:00.000Z").unwrap();
    assert_eq!(created.workspace.workspace_id, "wks_0123456789abcdef");
    assert_eq!(created.workspace.title.as_deref(), Some("Explicit title"));
    assert_eq!(created.workspace.auto_name, None);
    assert_eq!(created.project.project_id, "prj_selected");
    assert_eq!(
        workspaces.contexts.lock().unwrap()[0].expects_initial_agent,
        Some(true)
    );
    let state = managed.state.lock().unwrap();
    assert_eq!(state.created_inputs[0].cwd, "/repo");
    assert_eq!(state.created_inputs[0].slug, "review");
    assert_eq!(
        state.created_inputs[0].mode,
        WorktreeCreateMode::BranchOff {
            base_ref: Some("develop".to_owned()),
            branch_name: "feature/review".to_owned()
        }
    );
    drop(state);
    assert_eq!(
        adapter.create(&input, "later").unwrap_err().code,
        "invalid_request"
    );
    assert_eq!(managed.state.lock().unwrap().created_inputs.len(), 1);
}

#[test]
fn unified_creation_validates_sources_before_git_side_effects() {
    let projects = Projects::default();
    let mut archived = project("archived");
    archived.archived_at = Some("2026-01-01T00:00:00.000Z".to_owned());
    projects.0.lock().unwrap().push(archived);
    let managed = Managed::default();
    let adapter = WorkspaceWorktrees::new(Arc::new(Mutex::new(service(
        &projects,
        &Workspaces::default(),
        &managed,
    ))));
    let mut input = intent();
    input.cwd = None;
    assert_eq!(
        adapter.create(&input, "now").unwrap_err().code,
        "source_required"
    );
    input.project_id = Some("missing".to_owned());
    assert_eq!(
        adapter.create(&input, "now").unwrap_err().code,
        "unknown_project"
    );
    input.project_id = Some("archived".to_owned());
    input.cwd = Some("/repo".to_owned());
    assert_eq!(
        adapter.create(&input, "now").unwrap_err().code,
        "archived_project"
    );
    input.project_id = None;
    input.checkout_source = Some(model::workspace::worktrees::WorktreeChangeRequest {
        forge: Some("gitlab".into()),
        number: 1,
        project_path: None,
    });
    assert_eq!(
        adapter.create(&input, "now").unwrap_err().code,
        "unsupported_capability"
    );
    assert!(managed.state.lock().unwrap().created_inputs.is_empty());
}

#[test]
fn unified_creation_prefers_ref_name_and_honors_explicit_empty_title() {
    let projects = Projects::default();
    let managed = Managed::default();
    let adapter = WorkspaceWorktrees::new(Arc::new(Mutex::new(service(
        &projects,
        &Workspaces::default(),
        &managed,
    ))));
    let mut input = intent();
    input.ref_name = Some(" release ".to_owned());
    input.title = Some("  ".to_owned());
    let created = adapter.create(&input, "now").unwrap();
    assert_eq!(created.workspace.title, None);
    assert_eq!(created.workspace.auto_name, None);
    assert_eq!(
        managed.state.lock().unwrap().created_inputs[0].mode,
        WorktreeCreateMode::BranchOff {
            base_ref: Some("release".to_owned()),
            branch_name: "feature/review".to_owned()
        }
    );
}

#[test]
fn generated_titles_preserve_explicit_branches_and_only_rename_placeholders() {
    for (branch_name, expects_placeholder) in [(Some("feature/review"), false), (None, true)] {
        let managed = Managed::default();
        let adapter = WorkspaceWorktrees::new(Arc::new(Mutex::new(service(
            &Projects::default(),
            &Workspaces::default(),
            &managed,
        ))));
        let mut input = intent();
        input.title = None;
        input.worktree_slug = None;
        input.branch_name = branch_name.map(str::to_owned);
        let created = adapter.create(&input, "now").unwrap();
        let naming = created
            .workspace
            .auto_name
            .expect("provisional title can be generated");
        assert_eq!(naming.placeholder_branch.is_some(), expects_placeholder);
    }
}
