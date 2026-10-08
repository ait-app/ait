use super::fixture::Fixture;
use crate::ports::agent_session::{AgentClient, AgentResumePurpose};
use serde_json::json;

#[tokio::test]
async fn discovers_user_plugin_presets_and_separate_permissions_without_session_writes() {
    let fixture = Fixture::new().await;
    let details = fixture.client.discover(&fixture.spec.cwd).await.unwrap();
    assert_eq!(details.modes.len(), 1);
    assert_eq!(details.modes[0]["id"], "agent-preset:team/review.v2");
    assert_eq!(details.modes[0]["label"], "Team review");
    assert_eq!(details.modes[0]["isDefault"], true);
    assert_eq!(details.features[0]["id"], "permission_preset");
    assert!(
        details.features[0]["options"]
            .as_array()
            .unwrap()
            .iter()
            .any(|option| option["id"] == "custom-policy")
    );
    assert!(fixture.requests("session/create").is_empty());
    fixture.set_presets(json!({"presets":[{"id":"new-plugin","isDefault":true}]}));
    let refreshed = fixture.client.discover(&fixture.spec.cwd).await.unwrap();
    assert_eq!(refreshed.modes[0]["label"], "new-plugin");
    assert_eq!(refreshed.modes[0]["id"], "agent-preset:new-plugin");
    fixture.set_presets(json!({"presets":null}));
    assert!(fixture.client.discover(&fixture.spec.cwd).await.is_err());
}

#[tokio::test]
async fn creates_and_restores_plugin_composition_without_overwriting_permissions() {
    let mut fixture = Fixture::new().await;
    fixture.spec.config.mode_id = Some("agent-preset:team/review.v2".into());
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    assert_eq!(
        fixture.requests("session/create")[0]["request"]["agentPreset"],
        "team/review.v2"
    );
    assert!(fixture.requests("commands/execute").is_empty());
    assert_eq!(
        session.runtime_info().await.unwrap().mode_id,
        fixture.spec.config.mode_id
    );
    let controls = session.control_settings(&fixture.spec.config).unwrap();
    assert_eq!(controls["availableModes"].as_array().unwrap().len(), 1);
    assert_eq!(controls["features"][0]["value"], "workspace-write");
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let imported = fixture
        .client
        .inspect_session(&handle, &fixture.spec.cwd)
        .await
        .unwrap();
    assert_eq!(imported.config.mode_id, fixture.spec.config.mode_id);
    let mut resumed = fixture
        .client
        .resume_session(&handle, &fixture.spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    let mut changed = fixture.spec.config.clone();
    changed.mode_id = Some("agent-preset:another-plugin".into());
    assert!(resumed.validate_config_update(&changed).is_err());
    assert!(
        resumed
            .start_turn("wrong composition", &changed)
            .await
            .is_err()
    );
    assert!(fixture.requests("session/prompt").is_empty());
    changed = fixture.spec.config.clone();
    changed.feature_values = Some(std::collections::BTreeMap::from([(
        "permission_preset".into(),
        json!("custom-policy"),
    )]));
    resumed.start_turn("review", &changed).await.unwrap();
    assert_eq!(
        fixture.requests("commands/execute")[0]["line"],
        "/permission custom-policy"
    );
    assert_eq!(
        resumed.runtime_info().await.unwrap().mode_id,
        fixture.spec.config.mode_id
    );
    resumed.close().await.unwrap();
}

#[tokio::test]
async fn unknown_and_broken_presets_cannot_create_native_sessions() {
    let mut fixture = Fixture::new().await;
    for mode in ["agent-preset:unknown", "agent-preset:broken"] {
        fixture.spec.config.mode_id = Some(mode.into());
        assert!(fixture.client.create_session(&fixture.spec).await.is_err());
    }
    assert!(fixture.requests("session/create").is_empty());
    for value in [
        json!(true),
        json!("read-only\n/permission danger-full-access"),
        json!(""),
    ] {
        fixture.spec.config.mode_id = None;
        fixture.spec.config.feature_values = Some(std::collections::BTreeMap::from([(
            "permission_preset".into(),
            value,
        )]));
        assert!(
            fixture
                .client
                .validate_config(&fixture.spec.config)
                .is_err()
        );
    }
}

#[tokio::test]
async fn draft_permission_selection_is_read_only() {
    let mut fixture = Fixture::new().await;
    fixture.spec.config.feature_values = Some(std::collections::BTreeMap::from([(
        "permission_preset".into(),
        json!("read-only"),
    )]));
    let features = fixture.client.draft_features(&fixture.spec).await.unwrap();
    assert_eq!(features[0]["value"], "read-only");
    assert!(fixture.requests("session/create").is_empty());
    assert!(fixture.requests("commands/execute").is_empty());
}

#[tokio::test]
async fn custom_host_profile_is_passed_as_one_argument() {
    let fixture = Fixture::new().await;
    let client = fixture
        .client
        .clone()
        .with_native_profile("team-web".into());
    client.discover(&fixture.spec.cwd).await.unwrap();
    let arguments =
        std::fs::read_to_string(std::path::Path::new(&fixture.spec.cwd).join("dsh.args")).unwrap();
    assert_eq!(
        arguments.lines().take(2).collect::<Vec<_>>(),
        vec!["--profile", "team-web"]
    );
}

#[tokio::test]
async fn composition_without_permissions_does_not_invent_a_selector() {
    let fixture = Fixture::new().await;
    fixture.set_snapshot_fields(json!({"projections":{"values":{"agentPreset":"team/review.v2"}}}));
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    let controls = session.control_settings(&fixture.spec.config).unwrap();
    assert_eq!(controls["features"], json!([]));
    let mut config = fixture.spec.config.clone();
    config.feature_values = Some(std::collections::BTreeMap::from([(
        "permission_preset".into(),
        json!("read-only"),
    )]));
    assert!(session.start_turn("no authority", &config).await.is_err());
    assert!(fixture.requests("commands/execute").is_empty());
    session.close().await.unwrap();
}

#[tokio::test]
async fn selection_validation_checks_catalogs_without_creating_probe_sessions() {
    let mut fixture = Fixture::new().await;
    fixture.spec.config.mode_id = Some("agent-preset:team/review.v2".into());
    fixture.spec.config.thinking_option_id = Some("high".into());
    fixture
        .client
        .validate_selection(&fixture.spec)
        .await
        .unwrap();
    fixture.spec.config.thinking_option_id = Some("invented".into());
    assert!(
        fixture
            .client
            .validate_selection(&fixture.spec)
            .await
            .is_err()
    );
    fixture.spec.config.thinking_option_id = None;
    fixture.spec.config.mode_id = Some("agent-preset:broken".into());
    assert!(
        fixture
            .client
            .validate_selection(&fixture.spec)
            .await
            .is_err()
    );
    fixture.spec.config.mode_id = Some("read-only".into());
    fixture
        .client
        .validate_selection(&fixture.spec)
        .await
        .unwrap();
    fixture.spec.config.mode_id = Some("unknown-permission".into());
    assert!(
        fixture
            .client
            .validate_selection(&fixture.spec)
            .await
            .is_err()
    );
    fixture.spec.config.mode_id = None;
    fixture.spec.config.model = Some("unknown-model".into());
    assert!(
        fixture
            .client
            .validate_selection(&fixture.spec)
            .await
            .is_err()
    );
    assert!(fixture.requests("session/create").is_empty());
    assert!(fixture.requests("commands/execute").is_empty());
}
