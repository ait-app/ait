//! Current provider-catalog-session.test.ts request contracts (Paseo 30178c4).

use std::sync::Arc;

use super::*;

#[tokio::test]
async fn draft_features_use_the_proposed_settings_without_caching_or_creating_an_agent() {
    let root = tempfile::tempdir().unwrap();
    let probes = [Probe::new("codex")];
    let catalog = Catalog::default();
    for enabled in [true, false] {
        let reply = catalog.execute(&clients(&probes), &SessionEvents::default(),
            "provider.features.list.request", json!({"draftConfig":{
                "provider":"codex","cwd":root.path(),"model":" native-model ",
                "modeId":"read-only","thinkingOptionId":"high","featureValues":{"fast_mode":enabled}
            }})).await.unwrap();
        assert_eq!(reply["features"][0]["value"], enabled);
        assert_eq!(reply["provider"], "codex");
        assert!(reply["error"].is_null());
        assert!(chrono::DateTime::parse_from_rfc3339(reply["fetchedAt"].as_str().unwrap()).is_ok());
    }
    let state = probes[0].state.lock().unwrap();
    assert_eq!(state.draft_configs.len(), 2);
    assert_eq!(
        state.draft_configs[0].model.as_deref(),
        Some("native-model")
    );
    assert_eq!(state.draft_configs[0].mode_id.as_deref(), Some("read-only"));
    assert_eq!(
        state.draft_configs[0].thinking_option_id.as_deref(),
        Some("high")
    );
    assert!(state.discovery_cwds.is_empty());
    assert!(catalog.cache.lock().unwrap().snapshots.is_empty());
}

#[tokio::test]
async fn draft_feature_failures_are_inline_and_do_not_fabricate_feature_values() {
    let root = tempfile::tempdir().unwrap();
    let probe = Probe::new("codex");
    for (available, message) in [
        (Ok(false), "Provider executable is unavailable"),
        (
            Err(AgentSessionError::Failed),
            "Provider feature discovery failed",
        ),
    ] {
        probe.state.lock().unwrap().available = available;
        let response = Catalog::default()
            .execute(
                &clients(std::slice::from_ref(&probe)),
                &SessionEvents::default(),
                "provider.features.list.request",
                json!({"draftConfig":{"provider":"codex","cwd":root.path()}}),
            )
            .await
            .unwrap();
        assert_eq!(response["error"], message);
        assert!(response.get("features").is_none());
    }
    for (provider, cwd, message) in [
        (
            "missing",
            root.path().to_owned(),
            "Provider is not installed",
        ),
        (
            "codex",
            root.path().join("missing"),
            "Working directory is unavailable",
        ),
    ] {
        let response = Catalog::default()
            .execute(
                &clients(std::slice::from_ref(&probe)),
                &SessionEvents::default(),
                "provider.features.list.request",
                json!({"draftConfig":{"provider":provider,"cwd":cwd}}),
            )
            .await
            .unwrap();
        assert_eq!(response["error"], message);
        assert!(response.get("features").is_none());
    }
    assert!(probe.state.lock().unwrap().draft_configs.is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn native_codex_draft_features_discover_workflows_without_opening_a_thread() {
    let fixture = crate::test_support::Fixture::new();
    fixture.mode("workflows");
    let clients = BTreeMap::from([(
        "codex".to_owned(),
        Arc::new(fixture.client()) as Arc<dyn AgentClient>,
    )]);
    let catalog = Catalog::default();
    for model in [None, Some("default"), Some("  ")] {
        let response = catalog
            .execute(
                &clients,
                &SessionEvents::default(),
                "provider.features.list.request",
                json!({"draftConfig":{"provider":"codex","cwd":fixture.cwd,"model":model}}),
            )
            .await
            .unwrap();
        assert_eq!(response["features"], json!([]));
    }
    assert!(!fixture.cwd.join("native-requests.jsonl").exists());
    let response = catalog
        .execute(
            &clients,
            &SessionEvents::default(),
            "provider.features.list.request",
            json!({"draftConfig":{"provider":"codex","cwd":fixture.cwd,"model":"offline-model",
            "featureValues":{"fast_mode":true,"plan_mode":true}}}),
        )
        .await
        .unwrap();
    let features = response["features"].as_array().unwrap();
    assert_eq!(
        features
            .iter()
            .find(|feature| feature["id"] == "service_tier")
            .unwrap()["value"],
        "fast"
    );
    assert_eq!(
        features
            .iter()
            .find(|feature| feature["id"] == "plan_mode")
            .unwrap()["value"],
        true
    );
    assert!(fixture.requests().iter().all(|request| !matches!(
        request["method"].as_str(),
        Some("thread/start" | "thread/resume" | "turn/start")
    )));
    let unknown = catalog
        .execute(
            &clients,
            &SessionEvents::default(),
            "provider.features.list.request",
            json!({"draftConfig":{"provider":"codex","cwd":fixture.cwd,"model":"unknown-model"}}),
        )
        .await
        .unwrap();
    assert!(
        unknown["features"]
            .as_array()
            .unwrap()
            .iter()
            .all(|feature| feature["id"] != "service_tier")
    );
}

#[tokio::test]
async fn hidden_models_remain_in_snapshots_but_are_not_offered_in_the_model_picker() {
    let root = tempfile::tempdir().unwrap();
    let probes = [Probe::new("codex")];
    probes[0]
        .state
        .lock()
        .unwrap()
        .discovery
        .as_mut()
        .unwrap()
        .models
        .extend([
            json!({"id":"legacy","isSelectable":false}),
            json!({"id":"visible","isSelectable":true}),
        ]);
    let catalog = Catalog::default();
    let snapshot = snapshot(&catalog, &probes, root.path()).await;
    assert_eq!(
        snapshot["entries"][0]["models"].as_array().unwrap().len(),
        3
    );
    let response = catalog
        .execute(
            &clients(&probes),
            &SessionEvents::default(),
            "provider.models.list.request",
            json!({"provider":"codex","cwd":root.path()}),
        )
        .await
        .unwrap();
    let ids: Vec<_> = response["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|model| model["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["native-model", "visible"]);
}

#[tokio::test]
async fn missing_null_and_blank_cwd_use_a_distinct_global_snapshot() {
    let probes = [Probe::new("codex")];
    let catalog = Catalog::default();
    let events = SessionEvents::default();
    for params in [json!({}), json!({"cwd":null}), json!({"cwd":" \n "})] {
        let response = catalog
            .execute(
                &clients(&probes),
                &events,
                "provider.snapshot.get.request",
                params,
            )
            .await
            .unwrap();
        assert!(response.get("cwd").is_none());
    }
    assert_eq!(probes[0].state.lock().unwrap().discovery_cwds.len(), 1);
    let home = scope::directory("~").unwrap();
    let response = catalog
        .execute(
            &clients(&probes),
            &events,
            "provider.snapshot.get.request",
            json!({"cwd":"  ~  "}),
        )
        .await
        .unwrap();
    assert_eq!(response["cwd"], home);
    assert_eq!(catalog.cache.lock().unwrap().snapshots.len(), 2);
    assert_eq!(
        probes[0].state.lock().unwrap().discovery_cwds,
        [home.clone(), home]
    );
    let relative = catalog
        .execute(
            &clients(&probes),
            &events,
            "provider.snapshot.get.request",
            json!({"cwd":" . "}),
        )
        .await
        .unwrap();
    assert_eq!(
        relative["cwd"],
        std::env::current_dir()
            .unwrap()
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
    );
}

#[test]
fn scope_normalization_expands_home_children_and_rejects_files_and_missing_paths() {
    use super::super::super::scope;

    assert_eq!(
        scope::directory("~/.").unwrap(),
        scope::directory("~").unwrap()
    );
    let file = tempfile::NamedTempFile::new().unwrap();
    assert_eq!(
        scope::directory(file.path().to_str().unwrap()),
        Err(ErrorCode::InvalidMessage)
    );
    assert_eq!(
        scope::key(Some("/missing-paseo-catalog-path")),
        Err(ErrorCode::InvalidMessage)
    );
}
