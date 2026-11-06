use serde_json::{Value, json};

use super::*;

#[test]
fn records_match_pinned_paseo_zod_outputs_and_rejections() {
    let fixtures: Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/paseo-registry.json")).unwrap();
    for case in fixtures["cases"].as_array().unwrap() {
        let result = match case["schema"].as_str().unwrap() {
            "PersistedProjectRecord" => {
                serde_json::from_value::<PersistedProjectRecord>(case["input"].clone())
                    .and_then(serde_json::to_value)
            }
            "PersistedWorkspaceRecord" => {
                serde_json::from_value::<PersistedWorkspaceRecord>(case["input"].clone())
                    .and_then(serde_json::to_value)
            }
            schema => panic!("unexpected fixture schema {schema}"),
        };
        assert_eq!(
            result.is_ok(),
            case["valid"].as_bool().unwrap(),
            "{}: {result:?}",
            case["name"]
        );
        if let Ok(output) = result {
            assert_eq!(output, case["output"], "{}", case["name"]);
        }
    }
}

#[test]
fn explicit_names_override_derived_names_even_when_empty() {
    let mut project: PersistedProjectRecord = serde_json::from_value(json!({
        "projectId":"prj_one","rootPath":"/repo","kind":"git","displayName":"repo",
        "createdAt":"opaque","updatedAt":"opaque","archivedAt":null
    }))
    .unwrap();
    assert_eq!(project.display_name(), "repo");
    project.custom_name = Some(String::new());
    assert_eq!(project.display_name(), "");
    let mut workspace: PersistedWorkspaceRecord = serde_json::from_value(json!({
        "workspaceId":"wks_one","projectId":"prj_one","cwd":"/repo","kind":"local_checkout",
        "displayName":"main","createdAt":"opaque","updatedAt":"opaque","archivedAt":null
    }))
    .unwrap();
    assert_eq!(workspace.display_name(), "main");
    workspace.title = Some("Title".to_owned());
    assert_eq!(workspace.display_name(), "Title");
    workspace.title = Some(String::new());
    assert_eq!(workspace.display_name(), "");
}

#[test]
fn only_nonempty_archive_timestamps_mark_records_archived() {
    let mut project: PersistedProjectRecord = serde_json::from_value(json!({
        "projectId":"prj_one","rootPath":"/repo","kind":"git","displayName":"repo",
        "createdAt":"opaque","updatedAt":"opaque","archivedAt":null
    }))
    .unwrap();
    let mut workspace: PersistedWorkspaceRecord = serde_json::from_value(json!({
        "workspaceId":"wks_one","projectId":"prj_one","cwd":"/repo","kind":"local_checkout",
        "displayName":"main","createdAt":"opaque","updatedAt":"opaque","archivedAt":null
    }))
    .unwrap();
    for (archived_at, archived) in [(None, false), (Some(""), false), (Some("then"), true)] {
        project.archived_at = archived_at.map(str::to_owned);
        workspace.archived_at = archived_at.map(str::to_owned);
        assert_eq!(project.is_archived(), archived);
        assert_eq!(workspace.is_archived(), archived);
    }
}

#[test]
fn sandbox_level_is_optional_for_legacy_records_and_round_trips_when_present() {
    let legacy = json!({"workspaceId":"wks_1","projectId":"prj_1","cwd":"/wt","kind":"worktree",
        "displayName":"feature","createdAt":"c","updatedAt":"u","archivedAt":null});

    let record: PersistedWorkspaceRecord = serde_json::from_value(legacy.clone()).unwrap();
    assert_eq!(record.sandbox, None);
    assert!(
        serde_json::to_value(&record)
            .unwrap()
            .get("sandbox")
            .is_none()
    );

    let mut sandboxed = legacy;
    sandboxed["sandbox"] = json!("worktree_writes");
    let record: PersistedWorkspaceRecord = serde_json::from_value(sandboxed).unwrap();
    assert_eq!(
        record.sandbox,
        Some(crate::workspace::sandbox::WorkspaceSandbox::WorktreeWrites)
    );
    assert_eq!(
        serde_json::to_value(&record).unwrap()["sandbox"],
        "worktree_writes"
    );

    let mut unknown = serde_json::to_value(&record).unwrap();
    unknown["sandbox"] = json!("network_off");
    assert!(serde_json::from_value::<PersistedWorkspaceRecord>(unknown).is_err());
}
