//! Status attribution adapted from Paseo workspace-directory.test.ts.

use super::*;

fn snapshot(records: Vec<PersistedAgentRuntimeRecord>) -> Vec<WorkspaceActivity> {
    let agents = Agents::default();
    *agents.0.lock().unwrap() = records;
    AgentWorkspaceAttention::new(Box::new(agents))
        .snapshot()
        .unwrap()
}

fn child(id: &str, workspace: &str, parent: &str) -> PersistedAgentRuntimeRecord {
    let mut record = agent(id, workspace, "2026-09-29T00:00:00.000Z");
    record
        .labels
        .insert(PARENT_AGENT_ID_LABEL.to_owned(), parent.to_owned());
    record
}

#[test]
fn same_cwd_agents_attribute_status_only_to_their_own_workspace() {
    let mut running = agent("running", "wks-one", "2026-09-29T00:00:00.000Z");
    running.last_status = AgentRuntimeStatus::Running;
    let attention = agent("attention", "wks-two", "2026-09-29T01:00:00.000Z");
    let snapshot = snapshot(vec![running, attention]);
    assert!(
        snapshot.iter().any(|entry| entry.workspace_id == "wks-one"
            && entry.bucket == WorkspaceStateBucket::Running)
    );
    assert!(
        snapshot.iter().any(|entry| entry.workspace_id == "wks-two"
            && entry.bucket == WorkspaceStateBucket::Attention)
    );
}

#[test]
fn same_workspace_children_contribute_running_but_not_finished_attention() {
    let root = agent("root", "wks-one", "2026-09-29T00:00:00.000Z");
    let finished = child("finished", "wks-one", "root");
    let mut running = child("running", "wks-one", "root");
    running.last_status = AgentRuntimeStatus::Running;
    running.attention_reason = Some(AgentAttentionReason::Permission);
    let snapshot = snapshot(vec![root, finished, running]);
    assert_eq!(snapshot.len(), 2);
    assert!(
        snapshot
            .iter()
            .any(|entry| entry.bucket == WorkspaceStateBucket::Running)
    );
    assert!(
        !snapshot
            .iter()
            .any(|entry| entry.bucket == WorkspaceStateBucket::NeedsInput)
    );
}

#[test]
fn cross_workspace_children_contribute_their_full_bucket_to_their_own_workspace() {
    let root = agent("root", "wks-one", "2026-09-29T00:00:00.000Z");
    let mut delegated = child("child", "wks-two", "root");
    delegated.attention_reason = Some(AgentAttentionReason::Permission);
    let snapshot = snapshot(vec![root, delegated]);
    assert!(
        snapshot.iter().any(|entry| entry.workspace_id == "wks-two"
            && entry.bucket == WorkspaceStateBucket::NeedsInput)
    );
    assert!(
        snapshot.iter().any(|entry| entry.workspace_id == "wks-one"
            && entry.bucket == WorkspaceStateBucket::Attention)
    );
}

#[test]
fn archived_internal_or_unowned_agents_and_broken_parent_chains_do_not_contribute() {
    let mut archived = agent("archived", "wks-one", "now");
    archived.archived_at = Some("archived".to_owned());
    let mut internal = agent("internal", "wks-one", "now");
    internal.internal = true;
    let mut unowned = agent("unowned", "wks-one", "now");
    unowned.workspace_id = None;
    let cycle_a = child("a", "wks-one", "b");
    let cycle_b = child("b", "wks-one", "a");
    let orphan = child("orphan", "wks-one", "missing");
    let archived_child = child("archived-child", "wks-one", "archived");
    assert!(
        snapshot(vec![
            archived,
            internal,
            unowned,
            cycle_a,
            cycle_b,
            orphan,
            archived_child
        ])
        .is_empty()
    );
}

#[test]
fn bucket_precedence_matches_permission_error_running_attention_and_done() {
    let mut record = agent("agent", "wks", "2026-09-29T00:00:00.000Z");
    record.last_status = AgentRuntimeStatus::Running;
    record.attention_reason = Some(AgentAttentionReason::Permission);
    assert_eq!(
        snapshot(vec![record.clone()])[0].bucket,
        WorkspaceStateBucket::NeedsInput
    );
    record.attention_reason = Some(AgentAttentionReason::Error);
    assert_eq!(
        snapshot(vec![record.clone()])[0].bucket,
        WorkspaceStateBucket::Failed
    );
    record.attention_reason = None;
    assert_eq!(
        snapshot(vec![record.clone()])[0].bucket,
        WorkspaceStateBucket::Running
    );
    record.last_status = AgentRuntimeStatus::Error;
    assert_eq!(
        snapshot(vec![record.clone()])[0].bucket,
        WorkspaceStateBucket::Failed
    );
    record.last_status = AgentRuntimeStatus::Closed;
    assert_eq!(
        snapshot(vec![record.clone()])[0].bucket,
        WorkspaceStateBucket::Attention
    );
    record.requires_attention = false;
    assert_eq!(
        snapshot(vec![record.clone()])[0].bucket,
        WorkspaceStateBucket::Done
    );
}

#[test]
fn captures_source_failures_and_prefers_attention_timestamp_over_activity() {
    let agents = Agents::default();
    agents.2.lock().unwrap().list = true;
    let source = AgentWorkspaceAttention::new(Box::new(agents.clone()));
    assert_eq!(
        source.snapshot().unwrap_err(),
        WorkspaceStateError::AgentRegistry
    );
    agents.2.lock().unwrap().list = false;
    let mut record = agent("a", "wks", "2026-09-29T00:00:00.000Z");
    record.last_activity_at = Some("2026-09-29T00:05:00.000Z".to_owned());
    agents.upsert(&record).unwrap();
    assert_eq!(
        source.snapshot().unwrap()[0].changed_at.as_deref(),
        Some("2026-09-29T00:00:00.000Z")
    );
    record.attention_timestamp = None;
    agents.update("a", &|_| record.clone()).unwrap();
    assert_eq!(
        source.snapshot().unwrap()[0].changed_at.as_deref(),
        Some("2026-09-29T00:05:00.000Z")
    );
}

#[test]
fn running_native_children_contribute_to_the_active_workspace_root() {
    let agents = Agents::default();
    let root = agent("root", "wks-one", "2026-09-29T00:00:00.000Z");
    let delegated = child("delegated", "wks-two", "root");
    let mut archived = agent("archived", "wks-three", "2026-09-29T00:00:00.000Z");
    archived.archived_at = Some("archived".to_owned());
    *agents.0.lock().unwrap() = vec![root, delegated, archived];
    let timeline = crate::storage::timeline::Timeline::memory().unwrap();
    let mut native = crate::ports::controls::NativeSubagent {
        persistence: None,
        id: "native-child".to_owned(),
        parent_id: "native-parent".to_owned(),
        cwd: "/cwd".to_owned(),
        descriptor: serde_json::json!({"id":"native-child","status":"running","updatedAt":"2026-09-29T00:03:00.000Z"}),
    };
    for parent in ["root", "delegated", "archived", "missing"] {
        timeline.store_subagent(parent, &native).unwrap();
    }
    let source = AgentWorkspaceAttention::new(Box::new(agents)).with_timeline(timeline.clone());
    let running: Vec<_> = source
        .snapshot()
        .unwrap()
        .into_iter()
        .filter(|entry| entry.bucket == WorkspaceStateBucket::Running)
        .collect();
    assert_eq!(running.len(), 2);
    assert!(running.iter().any(|entry| entry.workspace_id == "wks-one"));
    assert!(running.iter().any(|entry| entry.workspace_id == "wks-two"));
    assert!(
        running
            .iter()
            .all(|entry| entry.changed_at.as_deref() == Some("2026-09-29T00:03:00.000Z"))
    );
    native.descriptor["status"] = serde_json::json!("completed");
    for parent in ["root", "delegated"] {
        timeline.store_subagent(parent, &native).unwrap();
    }
    assert!(
        source
            .snapshot()
            .unwrap()
            .iter()
            .all(|entry| entry.bucket != WorkspaceStateBucket::Running)
    );
}
