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
