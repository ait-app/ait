//! Paseo directory-sync and owned workspace subscription regression cases.

use super::*;
use crate::directory::rpc::directory::{execute, listing};
use model::ServerMessage;
use serde_json::{Value, json};

mod subscription_admission;

fn checkpoint(response: &Value) -> Value {
    json!({"generation":response["sync"]["generation"],"afterSeq":response["sync"]["headSeq"]})
}

fn add_workspace(directory: &Directory, id: &str) {
    let mut record = workspace();
    record.workspace_id = id.to_owned();
    directory
        .workspaces
        .upsert(&record, WorkspaceMutationContext::default())
        .unwrap();
}

fn events(events: Vec<ServerMessage>) -> Vec<Value> {
    events
        .into_iter()
        .map(|event| match event {
            ServerMessage::Event { method, params } => {
                assert_eq!(method, "workspace.update");
                params
            }
            _ => panic!("expected workspace event"),
        })
        .collect()
}

#[test]
fn project_reads_share_sequences_across_clones_and_report_removals() {
    let mut directory = directory();
    let first = execute(&mut directory, "project.list.request", json!({"sync":{}})).unwrap();
    assert_eq!(first["sync"]["reason"], "no_cursor");
    let mut clone = directory.clone();
    let unchanged = execute(
        &mut clone,
        "project.list.request",
        json!({"sync":checkpoint(&first)}),
    )
    .unwrap();
    assert_eq!(unchanged["projects"], json!([]));
    directory
        .rename_project("prj_a", Some("Renamed"), "now")
        .unwrap();
    let changed = execute(
        &mut clone,
        "project.list.request",
        json!({"sync":checkpoint(&first)}),
    )
    .unwrap();
    assert_eq!(changed["projects"][0]["projectDisplayName"], "Renamed");
    assert_eq!(changed["projects"][0]["syncSeq"], 2);
    directory.projects.remove("prj_a").unwrap();
    let removed = execute(
        &mut clone,
        "project.list.request",
        json!({"sync":checkpoint(&changed)}),
    )
    .unwrap();
    assert_eq!(removed["sync"]["removals"], json!([{"id":"prj_a","seq":3}]));
}

#[test]
fn synchronized_workspaces_are_unpaged_and_require_an_unfiltered_request() {
    let mut directory = directory();
    for index in 0..201 {
        add_workspace(&directory, &format!("wks_{index}"));
    }
    let first = execute(
        &mut directory,
        "workspace.list.request",
        json!({"sync":{},"page":{"limit":1}}),
    )
    .unwrap();
    assert_eq!(first["entries"].as_array().unwrap().len(), 202);
    assert_eq!(first["pageInfo"]["hasMore"], false);
    assert_eq!(first["emptyProjects"], json!([]));
    for params in [
        json!({"sync":{},"filter":{}}),
        json!({"sync":{},"page":{"limit":0}}),
    ] {
        assert!(execute(&mut directory, "workspace.list.request", params).is_err());
    }
    directory.archive_workspace("wks_a", "now").unwrap();
    let next = execute(
        &mut directory,
        "workspace.list.request",
        json!({"sync":checkpoint(&first)}),
    )
    .unwrap();
    assert_eq!(next["entries"], json!([]));
    assert_eq!(next["sync"]["removals"][0]["id"], "wks_a");
}

#[test]
fn workspace_subscription_bootstrap_tracks_rows_beyond_the_requested_page() {
    let directory = directory();
    add_workspace(&directory, "wks_b");
    let request = serde_json::from_value(json!({"subscribe":{},"page":{"limit":1}})).unwrap();
    let (first, mut observer) =
        listing::prepare(&directory, request, "subscription".to_owned()).unwrap();
    assert_eq!(first["entries"].as_array().unwrap().len(), 1);
    assert!(observer.update(&directory).unwrap().is_empty());
    directory
        .set_workspace_title("wks_b", Some("Changed outside page"), "now")
        .unwrap();
    let changed = events(observer.update(&directory).unwrap());
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0]["workspace"]["id"], "wks_b");
    assert_eq!(changed[0]["subscriptionId"], "subscription");
}

#[test]
fn subscription_filter_changes_emit_removal_and_reentry() {
    let directory = directory();
    let request =
        serde_json::from_value(json!({"subscribe":{},"filter":{"query":"main"}})).unwrap();
    let (_, mut observer) =
        listing::prepare(&directory, request, "subscription".to_owned()).unwrap();
    directory
        .set_workspace_title("wks_a", Some("Review"), "now")
        .unwrap();
    let removed = events(observer.update(&directory).unwrap());
    assert_eq!(removed[0]["kind"], "remove");
    assert_eq!(removed[0]["id"], "wks_a");
    assert!(removed[0].get("removedProjectId").is_none());
    directory
        .set_workspace_title("wks_a", None, "later")
        .unwrap();
    assert_eq!(
        events(observer.update(&directory).unwrap())[0]["kind"],
        "upsert"
    );
}

#[test]
fn archive_keeps_the_empty_project_and_project_removal_clears_it() {
    let directory = directory();
    let request = serde_json::from_value(json!({"subscribe":{}})).unwrap();
    let (_, mut observer) =
        listing::prepare(&directory, request, "subscription".to_owned()).unwrap();
    directory.archive_workspace("wks_a", "now").unwrap();
    let removed = events(observer.update(&directory).unwrap());
    assert_eq!(removed[0]["emptyProject"]["projectId"], "prj_a");
    directory.projects.remove("prj_a").unwrap();
    let removed = events(observer.update(&directory).unwrap());
    assert_eq!(removed[0]["removedProjectId"], "prj_a");
    assert!(observer.update(&directory).unwrap().is_empty());
}

#[test]
fn sequenced_workspace_streams_share_the_read_checkpoint() {
    let mut directory = directory();
    let request = serde_json::from_value(json!({"subscribe":{},"sync":{}})).unwrap();
    let (first, mut observer) =
        listing::prepare(&directory, request, "subscription".to_owned()).unwrap();
    directory
        .set_workspace_title("wks_a", Some("Renamed"), "now")
        .unwrap();
    let update = events(observer.update(&directory).unwrap());
    assert_eq!(update[0]["seq"], 2);
    assert_eq!(update[0]["generation"], first["sync"]["generation"]);
    let read = execute(
        &mut directory,
        "workspace.list.request",
        json!({"sync":checkpoint(&first)}),
    )
    .unwrap();
    assert_eq!(read["entries"][0]["syncSeq"], update[0]["seq"]);
    assert!(observer.update(&directory).unwrap().is_empty());
}

#[test]
fn modern_workspace_subscriptions_reject_client_assigned_identities() {
    let directory = directory();
    let request = serde_json::from_value(json!({"subscribe":{"subscriptionId":"client"}})).unwrap();
    assert!(listing::prepare(&directory, request, "server".to_owned()).is_err());
}
