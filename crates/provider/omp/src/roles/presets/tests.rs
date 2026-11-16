use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::*;
use crate::tests::{calls_after, fixture, fixture_with, with_variables};
use provider_core::ports::agent_session::AgentClient;

fn preset(name: &str) -> Preset {
    Preset {
        name: name.to_owned(),
        model: json!(["test", "model"]).to_string(),
        thinking_option_id: Some("high".to_owned()),
        feature_values: BTreeMap::from([("modelRole.task".to_owned(), json!("test/model:low"))]),
    }
}

#[tokio::test]
async fn native_presets_round_trip_without_a_session_or_global_role_change() {
    let (root, client, spec) = fixture();
    let descriptor = client.preset_descriptor(&spec.cwd).await;
    assert_eq!(descriptor.backend, PresetBackend::Native);
    assert!(descriptor.can_create && descriptor.can_edit && descriptor.can_delete);
    assert!(!descriptor.can_rename);
    let mut saved = preset("Team");
    saved
        .feature_values
        .insert("modelRole.task".to_owned(), json!("unset"));

    client.save_native_preset(&spec.cwd, &saved).await.unwrap();
    let listed = client.native_presets(&spec.cwd).await.unwrap();

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].preset, saved);
    assert_eq!(listed[0].source, PresetSource::Global);
    assert!(listed[0].editable);
    let stored: Value = serde_json::from_slice(
        &std::fs::read(root.path().join("agent/native-presets.json")).unwrap(),
    )
    .unwrap();
    assert!(stored["Team"]["modelRoles"]["task"].is_null());
    assert_eq!(stored["Team"]["modelRoles"]["default"], "test/model");
    let calls = calls_after(root.path(), 0);
    assert!(calls.iter().all(|call| call["args"][0] != "--mode"));
    assert!(
        calls
            .iter()
            .filter(|call| call["args"][1] == "set")
            .all(|call| call["args"][2] == "modelPresets")
    );

    client
        .delete_native_preset(&spec.cwd, "Team")
        .await
        .unwrap();
    assert!(client.native_presets(&spec.cwd).await.unwrap().is_empty());
    assert!(
        client
            .delete_native_preset(&spec.cwd, "Team")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn project_presets_override_whole_records_and_refuse_global_mutation() {
    let global = json!({"Team":{"modelRoles":{"default":"test/model","task":"test/model:high"},"defaultThinkingLevel":"high"}}).to_string();
    let project =
        json!({"Team":{"modelRoles":{"default":"test/model"},"defaultThinkingLevel":"low"}})
            .to_string();
    let (root, client, spec) = fixture_with(&[
        ("OMP_FIXTURE_PRESETS", &global),
        ("OMP_FIXTURE_PROJECT_PRESETS", &project),
    ]);

    let listed = client.native_presets(&spec.cwd).await.unwrap();

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].source, PresetSource::Project);
    assert!(!listed[0].editable);
    assert!(
        listed[0].preset.feature_values.is_empty(),
        "a project's record must not inherit role fields from the global same-name preset"
    );
    assert_eq!(listed[0].preset.thinking_option_id.as_deref(), Some("low"));
    assert!(
        client
            .save_native_preset(&spec.cwd, &preset("Team"))
            .await
            .is_err()
    );
    assert!(
        client
            .delete_native_preset(&spec.cwd, "Team")
            .await
            .is_err()
    );
    assert!(!root.path().join("agent/native-presets.json").exists());
}

#[tokio::test]
async fn cloned_clients_serialize_writes_without_losing_another_preset() {
    let (_root, client, spec) = fixture();
    let another = client.clone();
    let first = preset("First");
    let second = preset("Second");

    let (first, second) = tokio::join!(
        client.save_native_preset(&spec.cwd, &first),
        another.save_native_preset(&spec.cwd, &second),
    );

    first.unwrap();
    second.unwrap();
    let listed = client.native_presets(&spec.cwd).await.unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|entry| entry.preset.name.as_str())
            .collect::<Vec<_>>(),
        ["First", "Second"]
    );
}

#[tokio::test]
async fn invalid_models_roles_and_unrelated_features_never_reach_a_native_write() {
    let (root, client, spec) = fixture();
    let mut invalid = vec![preset("invalid name")];
    let mut unknown = preset("Unknown");
    unknown
        .feature_values
        .insert("modelRole.ghost".to_owned(), json!("test/model"));
    invalid.push(unknown);
    let mut unsupported = preset("Unsupported");
    unsupported
        .feature_values
        .insert("fastMode".to_owned(), json!(true));
    invalid.push(unsupported);
    let mut missing = preset("Missing");
    missing.model = json!(["test", "missing"]).to_string();
    invalid.push(missing);
    let mut bad_role = preset("WrongKind");
    bad_role
        .feature_values
        .insert("modelRole.task".to_owned(), json!("test/image"));
    invalid.push(bad_role);

    for value in invalid {
        assert!(
            client.save_native_preset(&spec.cwd, &value).await.is_err(),
            "{}",
            value.name
        );
    }

    assert!(!root.path().join("agent/native-presets.json").exists());
}

#[tokio::test]
async fn failed_or_unacknowledged_native_writes_are_reported() {
    for variable in [
        "OMP_FIXTURE_PRESET_WRITE_FAILS",
        "OMP_FIXTURE_PRESET_WRITE_IGNORED",
    ] {
        let (_root, client, spec) = fixture_with(&[(variable, "1")]);
        assert!(
            client
                .save_native_preset(&spec.cwd, &preset("Team"))
                .await
                .is_err()
        );
        assert!(client.native_presets(&spec.cwd).await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn preset_aliases_resolve_to_current_selectors_and_cycles_stay_unresolved() {
    let definitions =
        json!({"Team":{"modelRoles":{"default":"test/model:high","task":"@default"}}}).to_string();
    let (_root, client, spec) = fixture_with(&[("OMP_FIXTURE_PRESETS", &definitions)]);

    let listed = client.native_presets(&spec.cwd).await.unwrap();

    assert_eq!(listed[0].preset.model, json!(["test", "model"]).to_string());
    assert_eq!(listed[0].preset.thinking_option_id.as_deref(), Some("high"));
    assert_eq!(
        listed[0].preset.feature_values["modelRole.task"],
        "test/model:high"
    );
    let cycle = json!({"Team":{"modelRoles":{"default":"@task","task":"@default"}}}).to_string();
    let cycle_client = with_variables(&client, &[("OMP_FIXTURE_PRESETS", &cycle)]);
    let unresolved = cycle_client.native_presets(&spec.cwd).await.unwrap();
    assert_eq!(unresolved[0].preset.model, "@task");
    assert_eq!(
        unresolved[0].preset.feature_values["modelRole.task"],
        "@default"
    );
}

#[tokio::test]
async fn unresolved_native_entries_remain_editable_and_deletable_beside_valid_entries() {
    let definitions = json!({
        "Valid":{"modelRoles":{"default":"test/model","task":"test/model:low"}},
        "Retired":{"modelRoles":{"default":"test/retired:high","custom":"test/retired:low","future":true},"futureField":{"preserve":true}},
    })
    .to_string();
    let (root, client, spec) = fixture_with(&[("OMP_FIXTURE_PRESETS", &definitions)]);

    let listed = client.native_presets(&spec.cwd).await.unwrap();

    assert_eq!(listed.len(), 2);
    let mut retired = listed
        .iter()
        .find(|entry| entry.preset.name == "Retired")
        .unwrap()
        .preset
        .clone();
    assert_eq!(retired.model, json!(["test", "retired"]).to_string());
    assert_eq!(
        retired.feature_values["modelRole.custom"],
        "test/retired:low"
    );
    retired.thinking_option_id = Some("low".to_owned());
    client
        .save_native_preset(&spec.cwd, &retired)
        .await
        .unwrap();
    let after = client.native_presets(&spec.cwd).await.unwrap();
    assert_eq!(
        after
            .iter()
            .find(|entry| entry.preset.name == "Retired")
            .unwrap()
            .preset,
        retired
    );
    let stored: Value = serde_json::from_slice(
        &std::fs::read(root.path().join("agent/native-presets.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(stored["Retired"]["futureField"], json!({"preserve":true}));
    assert_eq!(stored["Retired"]["modelRoles"]["future"], true);
    client
        .delete_native_preset(&spec.cwd, "Retired")
        .await
        .unwrap();
    assert_eq!(
        client.native_presets(&spec.cwd).await.unwrap()[0]
            .preset
            .name,
        "Valid"
    );
}

#[tokio::test]
async fn global_preset_writes_invalidate_other_directories_role_catalogs() {
    let (root, client, spec) = fixture();
    let other = root.path().join("other");
    std::fs::create_dir(&other).unwrap();
    let other = other.to_str().unwrap();
    client.role_catalog(&spec.cwd).await.unwrap();
    client.role_catalog(other).await.unwrap();
    // An external native edit introduced a role after both directory caches were filled.
    let native = json!({"Team":{"modelRoles":{"default":"test/model","custom":"test/model"}}});
    std::fs::write(
        root.path().join("agent/native-presets.json"),
        serde_json::to_vec(&native).unwrap(),
    )
    .unwrap();
    let mut saved = preset("Team");
    saved
        .feature_values
        .insert("modelRole.custom".to_owned(), json!("test/model"));

    client.save_native_preset(&spec.cwd, &saved).await.unwrap();

    let other_catalog = client.role_catalog(other).await.unwrap();
    assert!(other_catalog.roles.iter().any(|role| role.id == "custom"));
    client
        .delete_native_preset(&spec.cwd, "Team")
        .await
        .unwrap();
    let other_catalog = client.role_catalog(other).await.unwrap();
    assert!(!other_catalog.roles.iter().any(|role| role.id == "custom"));
}

#[tokio::test]
async fn inherited_roles_remain_distinct_from_explicit_unassignment() {
    let (root, client, spec) = fixture();
    let mut inherited = preset("Inherited");
    inherited.thinking_option_id = None;
    inherited
        .feature_values
        .insert("modelRole.task".to_owned(), json!("native"));
    client
        .save_native_preset(&spec.cwd, &inherited)
        .await
        .unwrap();

    let listed = client.native_presets(&spec.cwd).await.unwrap();

    assert!(listed[0].preset.feature_values.is_empty());
    assert!(listed[0].preset.thinking_option_id.is_none());
    let stored: Value = serde_json::from_slice(
        &std::fs::read(root.path().join("agent/native-presets.json")).unwrap(),
    )
    .unwrap();
    assert!(stored["Inherited"]["modelRoles"].get("task").is_none());
    assert!(stored["Inherited"].get("defaultThinkingLevel").is_none());
}

#[tokio::test]
async fn a_native_snapshot_applies_through_the_existing_private_overlay() {
    let (root, client, mut spec) = fixture();
    let mut saved = preset("Team");
    saved
        .feature_values
        .insert("modelRole.task".to_owned(), json!("unset"));
    client.save_native_preset(&spec.cwd, &saved).await.unwrap();
    let listed = client.native_presets(&spec.cwd).await.unwrap();
    let selected = &listed[0].preset;
    spec.config.model = Some(selected.model.clone());
    spec.config
        .thinking_option_id
        .clone_from(&selected.thinking_option_id);
    spec.config.feature_values = Some(selected.feature_values.clone());

    let mut session = client.create_session(&spec).await.unwrap();

    let launch = calls_after(root.path(), 0)
        .into_iter()
        .find(|call| call["args"][0] == "--mode")
        .unwrap();
    let args = launch["args"].as_array().unwrap();
    let overlay = args
        .iter()
        .position(|argument| argument == "--config")
        .unwrap();
    let value: Value =
        serde_json::from_slice(&std::fs::read(args[overlay + 1].as_str().unwrap()).unwrap())
            .unwrap();
    assert_eq!(value, json!({"modelRoles":{"task":null}}));
    session.close().await.unwrap();
}

#[tokio::test]
async fn an_unreadable_native_catalog_never_claims_an_available_store() {
    let (_root, client, spec) = fixture_with(&[("OMP_FIXTURE_METADATA_FAILS", "1")]);

    let descriptor = client.preset_descriptor(&spec.cwd).await;

    assert_eq!(descriptor.backend, PresetBackend::Native);
    assert!(!descriptor.can_create && !descriptor.can_edit && !descriptor.can_delete);
    assert!(descriptor.reason.is_some());
    assert!(client.native_presets(&spec.cwd).await.is_err());
}
