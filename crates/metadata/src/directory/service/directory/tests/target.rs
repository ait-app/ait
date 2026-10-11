//! Identity-based reads and partial activity history do not need a full directory snapshot.

use domain::workspace::activity::WorkspaceStateBucket;
use domain::workspace::attention::{WorkspaceActivity, WorkspaceStateError};
use model::workspace::attention::WorkspaceActivitySource;
use serde_json::{Value, json};

use super::*;
use crate::directory::rpc::directory::execute;
use crate::support::error::ErrorCode;

fn get(directory: &mut Directory, id: &str) -> Value {
    execute(
        directory,
        "workspace.get.request",
        json!({"workspaceId":id}),
    )
    .unwrap()
}

#[test]
fn directory_target_returns_only_its_descriptor_and_owning_project() {
    let mut directory = directory();
    let mut other = workspace();
    other.workspace_id = "wks_b".into();
    other.cwd = "/tmp/other".into();
    directory
        .workspaces
        .upsert(&other, WorkspaceMutationContext::default())
        .unwrap();

    let result = get(&mut directory, "wks_a");
    assert_eq!(result["state"], "ready");
    assert_eq!(result["workspaceId"], "wks_a");
    assert_eq!(result["workspace"]["id"], "wks_a");
    assert_eq!(result["project"]["projectId"], "prj_a");
    let list = execute(&mut directory, "workspace.list.request", json!({})).unwrap();
    let listed = list["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == "wks_a")
        .unwrap();
    assert_eq!(&result["workspace"], listed);
}

#[test]
fn directory_target_distinguishes_missing_and_archived_without_restoring_records() {
    let mut directory = directory();
    assert_eq!(
        get(&mut directory, "missing"),
        json!({"workspaceId":"missing", "state":"missing", "workspace":null,"project":null})
    );
    directory.archive_workspace("wks_a", "archived").unwrap();
    let archived = get(&mut directory, "wks_a");
    assert_eq!(archived["state"], "archived");
    assert!(archived["workspace"].is_null());
    assert_eq!(
        directory
            .workspaces
            .get("wks_a")
            .unwrap()
            .unwrap()
            .archived_at
            .as_deref(),
        Some("archived")
    );

    let mut directory = super::directory();
    let mut project = project();
    project.archived_at = Some("archived".into());
    directory.projects.upsert(&project).unwrap();
    assert_eq!(get(&mut directory, "wks_a")["state"], "archived");
}

#[test]
fn directory_target_validates_exact_identity_and_propagates_registry_failures() {
    let mut directory = directory();
    for params in [
        json!({}),
        json!({"workspaceId":" "}),
        json!({"workspaceId":"x".repeat(257)}),
        json!({"workspaceId":"wks_a", "cwd":"/tmp"}),
    ] {
        assert_eq!(
            execute(&mut directory, "workspace.get.request", params).unwrap_err(),
            ErrorCode::InvalidMessage
        );
    }
    directory.projects = Arc::new(Projects {
        records: Mutex::new(vec![]),
        unavailable: true,
    });
    assert!(
        execute(
            &mut directory,
            "workspace.get.request",
            json!({"workspaceId":"wks_a"})
        )
        .is_err()
    );
}

#[derive(Debug)]
struct Running;

impl WorkspaceActivitySource for Running {
    fn snapshot(&self) -> Result<Vec<WorkspaceActivity>, WorkspaceStateError> {
        Ok(["wks_a", "wks_b"]
            .into_iter()
            .map(|workspace_id| WorkspaceActivity {
                workspace_id: workspace_id.into(),
                bucket: WorkspaceStateBucket::Running,
                changed_at: None,
            })
            .collect())
    }
}

#[test]
fn directory_target_preserves_other_workspace_transition_history() {
    let directory = directory().with_activity_source(Arc::new(Running));
    let mut other = workspace();
    other.workspace_id = "wks_b".into();
    let records = [workspace(), other];
    let first = directory.workspace_statuses(&records, "first").unwrap();
    let target = directory.workspace_status(&records[0], "target").unwrap();
    assert_eq!(target, first["wks_a"]);
    assert_eq!(
        directory.workspace_statuses(&records, "later").unwrap(),
        first
    );
}
