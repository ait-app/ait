//! Active Agent directory checkpoints and coherent subscription bootstraps.

use super::*;
use crate::rpc::agent_runtime::{execute, listing};
use serde_json::{Value, json};

fn checkpoint(response: &Value) -> Value {
    json!({"generation":response["sync"]["generation"],"afterSeq":response["sync"]["headSeq"]})
}

#[test]
fn synchronized_agent_reads_are_unpaged_and_report_changed_and_removed_entries() {
    let (mut directory, agents) = service();
    for index in 0..201 {
        agents
            .upsert(&agent(&format!("extra-{index}"), "wks-one", "Extra", false))
            .unwrap();
    }
    let first = execute(
        &mut directory,
        "agent.list.request",
        json!({"scope":"active","sync":{},"page":{"limit":1}}),
    )
    .unwrap();
    assert_eq!(first["entries"].as_array().unwrap().len(), 203);
    assert_eq!(first["pageInfo"]["hasMore"], false);
    directory
        .update("agent-a", Some("Changed"), None, "now")
        .unwrap();
    agents.remove("agent-b").unwrap();
    let next = execute(
        &mut directory,
        "agent.list.request",
        json!({"scope":"active","sync":checkpoint(&first)}),
    )
    .unwrap();
    assert_eq!(next["entries"].as_array().unwrap().len(), 1);
    assert_eq!(next["entries"][0]["agent"]["title"], "Changed");
    assert_eq!(next["sync"]["removals"][0]["id"], "agent-b");
    let unchanged = execute(
        &mut directory,
        "agent.list.request",
        json!({"scope":"active","sync":checkpoint(&next)}),
    )
    .unwrap();
    assert_eq!(unchanged["entries"], json!([]));
    assert_eq!(unchanged["sync"]["removals"], json!([]));
}

#[test]
fn sequenced_agent_reads_require_active_scope_without_filters() {
    let (mut directory, _) = service();
    for params in [
        json!({"sync":{}}),
        json!({"scope":"active","sync":{},"filter":{}}),
        json!({"scope":"active","sync":{},"page":{"limit":201}}),
    ] {
        assert!(execute(&mut directory, "agent.list.request", params).is_err());
    }
}

#[test]
fn agent_subscription_bootstraps_keep_all_matching_rows_behind_a_bounded_page() {
    let (directory, _) = service();
    let request = serde_json::from_value(json!({"subscribe":{},"page":{"limit":1}})).unwrap();
    let prepared = listing::prepare(&directory, request)
        .unwrap()
        .finish(&directory)
        .unwrap();
    assert_eq!(prepared["response"]["entries"].as_array().unwrap().len(), 1);
    assert_eq!(prepared["entries"].as_array().unwrap().len(), 2);
    let request = serde_json::from_value(json!({"subscribe":{"subscriptionId":"client"}})).unwrap();
    assert!(listing::prepare(&directory, request).is_err());
}

#[test]
fn shared_directory_owner_keeps_the_same_generation_for_agent_reads() {
    let (directory, _) = service();
    let sync = model::directory_sync::DirectorySync::new("host".to_owned());
    let mut directory = directory.with_directory_sync(sync.clone());
    let first = execute(
        &mut directory,
        "agent.list.request",
        json!({"scope":"active","sync":{}}),
    )
    .unwrap();
    let projects = sync.synchronize("projects", [], &domain::directory_sync::Cursor::default());
    assert_eq!(first["sync"]["generation"], projects.sync.generation);
    assert_eq!(projects.sync.head_seq, 0);
}
