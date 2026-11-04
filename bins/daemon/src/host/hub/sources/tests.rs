use super::*;
use crate::host::hub::tests::sources;
use domain::agent_runtime::PersistedAgentRuntimeRecord;
use domain::workspace::records::PersistedWorkspaceRecord;

#[test]
fn lightweight_agent_projection_hides_internal_archived_and_provider_configuration() {
    let directory = tempfile::tempdir().unwrap();
    let sources = sources(directory.path());
    let mut record: PersistedAgentRuntimeRecord = serde_json::from_value(json!({
        "id":"agent","provider":"test","cwd":"/private/path","createdAt":"now","updatedAt":"now",
        "workspaceId":"workspace","title":"Task","requiresAttention":true,
        "attentionReason":"finished","attentionTimestamp":"now",
        "config":{"systemPrompt":"secret"},
        "runtimeInfo":{"provider":"test","sessionId":"private-native-session"}
    }))
    .unwrap();
    record.attention_timestamp = Some(chrono::Utc::now().to_rfc3339());
    sources.agents.upsert(&record).unwrap();
    let snapshots = sources.snapshots().unwrap();
    assert_eq!(snapshots[2].1[0]["id"], "agent");
    assert_eq!(snapshots[2].1[0]["data"]["title"], "Task");
    assert!(!snapshots[2].1.to_string().contains("secret"));
    assert!(!snapshots[2].1.to_string().contains("private"));
    assert_eq!(
        sources
            .attention(chrono::Utc::now().timestamp_millis())
            .unwrap()
            .len(),
        1
    );
    record.internal = true;
    sources.agents.upsert(&record).unwrap();
    assert_eq!(sources.snapshots().unwrap()[2].1, json!([]));
    assert!(
        sources
            .attention(chrono::Utc::now().timestamp_millis())
            .unwrap()
            .is_empty()
    );
    record.internal = false;
    record.archived_at = Some("now".to_owned());
    sources.agents.upsert(&record).unwrap();
    assert_eq!(sources.snapshots().unwrap()[2].1, json!([]));
    record.archived_at = None;
    record.attention_timestamp = Some("2020-01-01T00:00:00Z".to_owned());
    sources.agents.upsert(&record).unwrap();
    assert!(
        sources
            .attention(chrono::Utc::now().timestamp_millis())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn project_workspace_renames_and_archives_replace_lightweight_snapshots() {
    let directory = tempfile::tempdir().unwrap();
    let sources = sources(directory.path());
    let mut project: domain::workspace::records::PersistedProjectRecord =
        serde_json::from_value(json!({
            "projectId":"project","rootPath":directory.path(),"kind":"git","displayName":"Derived",
            "customName":"Project","createdAt":"now","updatedAt":"now","archivedAt":null
        }))
        .unwrap();
    let mut workspace: PersistedWorkspaceRecord = serde_json::from_value(json!({
        "workspaceId":"workspace","projectId":"project","cwd":directory.path(),
        "kind":"directory","displayName":"Derived",
        "title":"Workspace","branch":"main","createdAt":"now","updatedAt":"now","archivedAt":null
    }))
    .unwrap();
    sources.projects.upsert(&project).unwrap();
    sources
        .workspaces
        .upsert(
            &workspace,
            domain::workspace::registry::WorkspaceMutationContext::default(),
        )
        .unwrap();
    let snapshots = sources.snapshots().unwrap();
    assert_eq!(snapshots[0].1[0]["data"]["name"], "Project");
    assert_eq!(snapshots[1].1[0]["data"]["title"], "Workspace");
    assert!(snapshots[0].1[0]["data"].get("rootPath").is_none());
    assert!(snapshots[1].1[0]["data"].get("cwd").is_none());
    // Imported Paseo records may store an empty archive marker for active entries.
    project.archived_at = Some(String::new());
    workspace.archived_at = Some(String::new());
    sources.projects.upsert(&project).unwrap();
    sources
        .workspaces
        .upsert(
            &workspace,
            domain::workspace::registry::WorkspaceMutationContext::default(),
        )
        .unwrap();
    let snapshots = sources.snapshots().unwrap();
    assert_eq!(snapshots[0].1[0]["id"], "project");
    assert_eq!(snapshots[1].1[0]["id"], "workspace");
    project.archived_at = Some("later".to_owned());
    workspace.archived_at = Some("later".to_owned());
    sources.projects.upsert(&project).unwrap();
    sources
        .workspaces
        .upsert(
            &workspace,
            domain::workspace::registry::WorkspaceMutationContext::default(),
        )
        .unwrap();
    let snapshots = sources.snapshots().unwrap();
    assert_eq!(snapshots[0].1, json!([]));
    assert_eq!(snapshots[1].1, json!([]));
}

fn item(id: usize, updated: &str, bytes: usize) -> Value {
    json!({"id":id,"data":{"title":"x".repeat(bytes),"updatedAt":updated}})
}

#[test]
fn directories_within_budget_are_published_unchanged() {
    let items = vec![item(2, "2026-01-01", 1), item(1, "2026-02-01", 1)];
    assert_eq!(fit_budget("agent", items.clone()), Value::from(items));
}

#[test]
fn oversized_directories_keep_the_most_recently_updated_items() {
    let items = (0..ITEM_LIMIT + 5)
        .map(|id| item(id, &format!("2026-01-01T00:00:{id:05}Z"), 1))
        .collect::<Vec<_>>();
    let published = fit_budget("agent", items);
    let published = published.as_array().unwrap();
    assert_eq!(published.len(), ITEM_LIMIT);
    assert_eq!(published[0]["id"], ITEM_LIMIT + 4);
    assert!(
        published
            .iter()
            .all(|item| item["id"].as_u64().unwrap() >= 5)
    );
    let mixed = vec![
        item(1, "2026-01-02", ITEM_BYTES + 1),
        item(2, "2026-01-01", 1),
    ];
    assert_eq!(
        fit_budget("agent", mixed),
        json!([item(2, "2026-01-01", 1)])
    );
    let heavy = (0..80)
        .map(|id| item(id, &format!("2026-01-01T00:00:{id:05}Z"), ITEM_BYTES - 64))
        .collect::<Vec<_>>();
    let published = fit_budget("agent", heavy);
    let encoded = serde_json::to_vec(&published).unwrap().len();
    assert!(encoded <= KIND_BYTES, "{encoded}");
    assert_eq!(published[0]["id"], 79);
}
