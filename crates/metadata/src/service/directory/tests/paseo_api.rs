//! Workspace list cases adapted from Paseo workspace-directory and sortable-pager tests.

use std::sync::Arc;

use model::workspace::activity::WorkspaceStateBucket;
use model::workspace::attention::{
    WorkspaceActivity, WorkspaceActivitySource, WorkspaceStateError,
};
use serde_json::{Value, json};

use super::*;
use crate::rpc::directory::execute;

#[derive(Debug, Default)]
struct Activity(Mutex<Vec<WorkspaceActivity>>);

#[test]
fn explicit_creation_identity_cannot_overwrite_an_existing_workspace() {
    let directory = directory();
    let before = directory.workspaces.get("wks_a").unwrap().unwrap();
    let created = directory.create_workspace(model::workspace::lifecycle::WorkspaceCreation {
        path: "/tmp/alpha/nested",
        title: Some("Overwrite attempt".into()),
        project_id: None,
        workspace_id: Some("wks_a".into()),
        expects_initial_agent: true,
        timestamp: "later",
    });
    assert!(created.is_err());
    assert_eq!(directory.workspaces.get("wks_a").unwrap(), Some(before));
    assert_eq!(directory.projects.list().unwrap().len(), 1);
}

#[test]
fn completed_workspace_validation_keeps_archived_records_but_rejects_absent_identities() {
    let directory = directory();
    assert!(directory.contains_workspace_directory("wks_a").unwrap());
    directory.archive_workspace("wks_a", "archived").unwrap();
    assert!(directory.contains_workspace_directory("wks_a").unwrap());
    assert!(!directory.contains_workspace_directory("missing").unwrap());
}

#[test]
fn multiple_activity_owners_contribute_without_replacing_each_other() {
    let first = Arc::new(Activity::default());
    let second = Arc::new(Activity::default());
    first.0.lock().unwrap().push(WorkspaceActivity {
        workspace_id: "wks_a".into(),
        bucket: WorkspaceStateBucket::Running,
        changed_at: None,
    });
    second.0.lock().unwrap().push(WorkspaceActivity {
        workspace_id: "wks_b".into(),
        bucket: WorkspaceStateBucket::NeedsInput,
        changed_at: None,
    });
    let mut directory = directory()
        .with_activity_source(first)
        .with_activity_source(second);
    add_workspace(&directory, "wks_b", "other activity owner");
    let result = list(&mut directory, json!({}));
    let entries = result["entries"].as_array().unwrap();
    assert_eq!(
        entries.iter().find(|entry| entry["id"] == "wks_a").unwrap()["status"],
        "running"
    );
    assert_eq!(
        entries.iter().find(|entry| entry["id"] == "wks_b").unwrap()["status"],
        "needs_input"
    );
}

impl WorkspaceActivitySource for Activity {
    fn snapshot(&self) -> Result<Vec<WorkspaceActivity>, WorkspaceStateError> {
        Ok(self.0.lock().unwrap().clone())
    }
}

#[test]
fn projects_only_owned_worktree_slugs() {
    let mut record = workspace();
    let descriptor =
        model::workspace::protocol::projection::workspace_descriptor(&record, Some(&project()));
    assert!(descriptor.worktree_slug.is_none());
    record.is_paseo_owned_worktree = true;
    assert_eq!(
        model::workspace::protocol::projection::workspace_descriptor(&record, Some(&project()))
            .worktree_slug
            .as_deref(),
        Some("alpha")
    );
}

#[test]
fn lists_activity_by_workspace_identity_before_status_sorting() {
    let activity = Arc::new(Activity::default());
    activity.0.lock().unwrap().push(WorkspaceActivity {
        workspace_id: "wks_b".to_owned(),
        bucket: WorkspaceStateBucket::Running,
        changed_at: Some("2026-09-29T00:00:00.000Z".to_owned()),
    });
    let mut directory = directory().with_activity_source(activity);
    add_workspace(&directory, "wks_b", "same checkout");
    let result = list(
        &mut directory,
        json!({"sort":[{"key":"status_priority","direction":"asc"}]}),
    );
    assert_eq!(result["entries"][0]["id"], "wks_b");
    assert_eq!(result["entries"][0]["status"], "running");
    assert_eq!(
        result["entries"][0]["statusEnteredAt"],
        "2026-09-29T00:00:00.000Z"
    );
    assert_eq!(result["entries"][1]["status"], "done");
    assert_eq!(result["entries"][1]["statusEnteredAt"], "created");
}

#[test]
fn higher_priority_activity_masks_lower_buckets_and_unmasking_uses_now() {
    let activity = Arc::new(Activity::default());
    let records = vec![workspace()];
    let directory = directory().with_activity_source(activity.clone());
    let moment = "2026-09-29T00:00:00.000Z";
    for bucket in [
        WorkspaceStateBucket::Done,
        WorkspaceStateBucket::Attention,
        WorkspaceStateBucket::Running,
        WorkspaceStateBucket::Failed,
        WorkspaceStateBucket::NeedsInput,
    ] {
        activity.0.lock().unwrap().push(WorkspaceActivity {
            workspace_id: "wks_a".to_owned(),
            bucket,
            changed_at: Some(moment.to_owned()),
        });
    }
    let first = directory.workspace_statuses(&records, "initial").unwrap();
    assert_eq!(first["wks_a"].bucket, WorkspaceStateBucket::NeedsInput);
    assert_eq!(first["wks_a"].entered_at, moment);
    activity.0.lock().unwrap().pop();
    let second = directory.workspace_statuses(&records, "unmasked").unwrap();
    assert_eq!(second["wks_a"].bucket, WorkspaceStateBucket::Failed);
    assert_eq!(second["wks_a"].entered_at, "unmasked");
    let stable = directory.workspace_statuses(&records, "later").unwrap();
    assert_eq!(stable["wks_a"].entered_at, "unmasked");
    activity.0.lock().unwrap().clear();
    assert_eq!(
        directory.workspace_statuses(&records, "empty").unwrap()["wks_a"].entered_at,
        "empty"
    );
    directory.workspace_statuses(&[], "archived").unwrap();
    assert_eq!(
        directory.workspace_statuses(&records, "restored").unwrap()["wks_a"].entered_at,
        "created"
    );
}

#[test]
fn initial_activity_uses_the_newest_winning_timestamp_or_now_when_invalid() {
    let activity = Arc::new(Activity::default());
    for timestamp in ["2026-09-29T00:00:00.000Z", "2026-09-29T00:02:00.000Z"] {
        activity.0.lock().unwrap().push(WorkspaceActivity {
            workspace_id: "wks_a".to_owned(),
            bucket: WorkspaceStateBucket::Running,
            changed_at: Some(timestamp.to_owned()),
        });
    }
    let directory = directory().with_activity_source(activity.clone());
    assert_eq!(
        directory.workspace_statuses(&[workspace()], "now").unwrap()["wks_a"].entered_at,
        "2026-09-29T00:02:00.000Z"
    );
    directory.workspace_statuses(&[], "reset").unwrap();
    activity
        .0
        .lock()
        .unwrap()
        .iter_mut()
        .for_each(|activity| activity.changed_at = Some("invalid".to_owned()));
    assert_eq!(
        directory.workspace_statuses(&[workspace()], "now").unwrap()["wks_a"].entered_at,
        "now"
    );
}

fn list(directory: &mut Directory, params: Value) -> Value {
    execute(directory, "workspace.list.request", params).unwrap()
}

fn add_workspace(directory: &Directory, id: &str, name: &str) {
    let mut record = workspace();
    record.workspace_id = id.to_owned();
    record.display_name = name.to_owned();
    directory
        .workspaces
        .upsert(&record, WorkspaceMutationContext::default())
        .unwrap();
}

#[test]
fn archived_projects_hide_their_unarchived_workspaces() {
    let mut directory = directory();
    directory.projects.archive("prj_a", "archived").unwrap();
    let result = list(&mut directory, json!({}));
    assert_eq!(result["entries"], json!([]));
    assert_eq!(result["emptyProjects"], json!([]));
}

#[test]
fn workspace_queries_match_name_and_identities_and_trim_project_filters() {
    let mut directory = directory();
    for query in [" MAIN ", "WKS_A", "PRJ_A"] {
        let result = list(
            &mut directory,
            json!({"filter":{"query":query,"projectId":" prj_a "}}),
        );
        assert_eq!(result["entries"].as_array().unwrap().len(), 1, "{query}");
    }
    for project_id in ["", " "] {
        assert_eq!(
            list(&mut directory, json!({"filter":{"projectId":project_id}}))["entries"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    assert_eq!(
        list(&mut directory, json!({"filter":{"query":"/tmp/alpha"}}))["entries"],
        json!([])
    );
}

#[test]
fn empty_projects_are_not_hidden_by_the_workspace_text_query() {
    let mut directory = directory();
    directory.archive_workspace("wks_a", "archived").unwrap();
    let result = list(&mut directory, json!({"filter":{"query":"unmatched"}}));
    assert_eq!(result["entries"], json!([]));
    assert_eq!(result["emptyProjects"][0]["projectId"], "prj_a");
    assert_eq!(
        list(&mut directory, json!({"filter":{"projectId":"other"}}))["emptyProjects"],
        json!([])
    );
}

#[test]
fn default_workspace_page_is_bounded_to_two_hundred_entries() {
    let mut directory = directory();
    for index in 0..201 {
        add_workspace(&directory, &format!("wks_{index:03}"), "same");
    }
    let result = list(&mut directory, json!({}));
    assert_eq!(result["entries"].as_array().unwrap().len(), 200);
    assert_eq!(result["pageInfo"]["hasMore"], true);
    let next = list(
        &mut directory,
        json!({"page":{"limit":200,"cursor":result["pageInfo"]["nextCursor"]}}),
    );
    assert_eq!(next["entries"].as_array().unwrap().len(), 2);
    assert_eq!(next["pageInfo"]["hasMore"], false);
}

#[test]
fn workspace_sort_is_case_insensitive_and_uses_identity_for_ties() {
    let mut directory = directory();
    add_workspace(&directory, "wks_z", "Beta");
    add_workspace(&directory, "wks_c", "alpha");
    add_workspace(&directory, "wks_b", "ALPHA");
    let result = list(
        &mut directory,
        json!({"sort":[{"key":"name","direction":"asc"}]}),
    );
    let ids = result["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(ids, ["wks_b", "wks_c", "wks_z", "wks_a"]);
}

#[test]
fn deleting_an_earlier_row_does_not_skip_the_next_workspace_page() {
    let mut directory = directory();
    add_workspace(&directory, "wks_b", "next");
    add_workspace(&directory, "wks_c", "last");
    let first = list(&mut directory, json!({"page":{"limit":1}}));
    assert_eq!(first["entries"][0]["id"], "wks_a");
    directory.archive_workspace("wks_a", "archived").unwrap();
    let cursor = &first["pageInfo"]["nextCursor"];
    let second = list(&mut directory, json!({"page":{"limit":1,"cursor":cursor}}));
    assert_eq!(second["entries"][0]["id"], "wks_b");
    assert_eq!(second["pageInfo"]["prevCursor"], *cursor);
}

#[test]
fn workspace_cursor_rejects_a_different_sort_order() {
    let mut directory = directory();
    add_workspace(&directory, "wks_b", "other");
    let first = list(&mut directory, json!({"page":{"limit":1}}));
    assert!(
        execute(
            &mut directory,
            "workspace.list.request",
            json!({
                "sort":[{"key":"name","direction":"asc"}],
                "page":{"limit":1,"cursor":first["pageInfo"]["nextCursor"]}
            })
        )
        .is_err()
    );
}
