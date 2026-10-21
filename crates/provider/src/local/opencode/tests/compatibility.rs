use super::*;

#[cfg(unix)]
pub(super) async fn release_after_old_deadline(root: &TempDir, phase: &str) {
    let entered = root.path().join(format!("{phase}.entered"));
    let release = root.path().join(format!("{phase}.release"));
    let wait_until = std::time::Instant::now() + Duration::from_secs(5);
    while !entered.exists() {
        assert!(
            std::time::Instant::now() < wait_until,
            "fixture did not reach {phase}"
        );
        tokio::task::yield_now().await;
    }
    tokio::time::advance(Duration::from_secs(31)).await;
    tokio::time::resume();
    std::fs::write(release, "ready").unwrap();
}

#[cfg(unix)]
#[tokio::test(start_paused = true)]
async fn native_initialize_can_complete_after_the_old_thirty_second_limit() {
    let (root, client, spec) = fixture("slow-initialize");
    let pending = tokio::spawn(async move { client.discover(&spec.cwd).await });
    release_after_old_deadline(&root, "initialize").await;
    let details = pending.await.unwrap().unwrap();
    assert_eq!(details.models.len(), 2);
    assert!(!root.path().join("native-fixture.json").exists());
}

#[cfg(unix)]
#[tokio::test(start_paused = true)]
async fn native_session_creation_can_complete_after_the_old_thirty_second_limit() {
    let (root, client, spec) = fixture("slow-session");
    let pending = tokio::spawn(async move { client.discover(&spec.cwd).await });
    release_after_old_deadline(&root, "session").await;
    let details = pending.await.unwrap().unwrap();
    assert_eq!(details.models.len(), 2);
    assert!(!root.path().join("native-fixture.json").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn broken_v2_catalog_releases_report_an_upgrade_before_creating_native_sessions() {
    for version in ["2.0.20", "2.0.24"] {
        let (root, mut client, spec) = fixture("normal");
        client
            .environment
            .insert("AIT_ACP_VERSION".into(), version.into());

        assert!(!client.is_available().await.unwrap());
        let diagnostic = client.diagnostic().await.unwrap();
        assert!(diagnostic.contains(version));
        assert!(diagnostic.contains("Upgrade OpenCode to 2.0.25 or newer"));
        assert!(matches!(
            client.create_session(&spec).await,
            Err(AgentSessionError::Unavailable)
        ));
        assert_eq!(
            client.discover(&spec.cwd).await.unwrap_err(),
            AgentSessionError::Unavailable
        );
        assert!(!root.path().join("launch.jsonl").exists());
        assert!(!root.path().join("native-fixture.json").exists());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn discovery_includes_late_models_and_omits_removed_native_models() {
    for scenario in ["changing-catalog", "changing-catalog-notification"] {
        let (root, client, spec) = fixture(scenario);

        let details = client.discover(&spec.cwd).await.unwrap();

        assert_eq!(
            details
                .models
                .iter()
                .map(|model| model["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["local/model", "local/third"]
        );
        assert_eq!(details.models[0]["label"], "Renamed model");
        assert_eq!(details.models[1]["thinkingOptions"][1]["id"], "high");
        assert_eq!(details.modes[0]["id"], "review");
        assert!(
            requests(&root)
                .iter()
                .any(|request| request["method"] == "session/set_config_option"
                    && request["params"]["value"] == "local/third")
        );
        assert!(!root.path().join("native-fixture.json").exists());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn broken_opencode_release_is_unavailable_with_upgrade_guidance_in_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let binary =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/opencode_acp.py");
    let mut client = OpenCodeClient::new(binary);
    client.environment = BTreeMap::from([
        (
            "AIT_ACP_FIXTURE_ROOT".into(),
            root.path().to_string_lossy().into_owned(),
        ),
        ("AIT_ACP_VERSION".into(), "2.0.20".into()),
    ]);

    let clients = BTreeMap::from([(
        "opencode".to_owned(),
        std::sync::Arc::new(client) as std::sync::Arc<dyn AgentClient>,
    )]);
    let snapshot = crate::service::provider_catalog::Catalog::default()
        .execute(
            &clients,
            &model::session::SessionEvents::default(),
            "provider.snapshot.get.request",
            json!({"cwd":root.path()}),
        )
        .await
        .unwrap();
    let entry = &snapshot["entries"][0];

    assert_eq!(entry["status"], "unavailable");
    assert!(
        entry["error"]
            .as_str()
            .unwrap()
            .contains("Upgrade OpenCode to 2.0.25 or newer")
    );
    assert_eq!(entry["models"], json!([]));
    assert!(!root.path().join("launch.jsonl").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn native_selection_confirmation_before_or_after_response_precedes_the_prompt() {
    for scenario in ["stale-selection-before", "stale-selection-after"] {
        let (root, client, mut spec) = fixture(scenario);
        spec.config.model = Some("local/second".into());
        spec.config.thinking_option_id = Some("high".into());
        spec.config.mode_id = Some("plan".into());

        let mut session = client.create_session(&spec).await.unwrap();
        let info = session.runtime_info().await.unwrap();
        assert_eq!(info.model, spec.config.model);
        assert_eq!(info.thinking_option_id, spec.config.thinking_option_id);
        assert_eq!(info.mode_id, spec.config.mode_id);
        session.start_turn("hello", &spec.config).await.unwrap();
        completed(session.as_mut()).await;
        session.close().await.unwrap();
        let details = client.discover(&spec.cwd).await.unwrap();
        assert_eq!(details.models.len(), 2);
        let log = requests(&root);
        assert_eq!(
            log.iter()
                .filter(|request| request["method"] == "session/prompt")
                .count(),
            1
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn missing_or_foreign_configuration_confirmation_fails_without_submitting_a_prompt() {
    for scenario in ["stale-selection-missing", "stale-selection-wrong"] {
        let (root, mut client, mut spec) = fixture(scenario);
        client.deadline = Duration::from_secs(1);
        spec.config.model = Some("local/second".into());

        assert!(matches!(
            client.create_session(&spec).await,
            Err(AgentSessionError::Failed)
        ));

        let log = requests(&root);
        assert_eq!(
            log.iter()
                .filter(|request| request["method"] == "session/set_config_option")
                .count(),
            1
        );
        assert!(
            !log.iter()
                .any(|request| request["method"] == "session/prompt")
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn native_startup_is_coordinated_across_clones_without_locking_live_sessions() {
    let (root, client, spec) = fixture("startup-overlap");
    let cloned = client.clone();
    let config = StoredAgentConfig::default();
    let (first, second) = tokio::join!(
        launcher::spawn(&client, &spec.cwd, &config),
        launcher::spawn(&cloned, &spec.cwd, &config),
    );
    // Both connections must initialize while the first connection is still alive.
    let (mut first, _) = first.unwrap();
    let (mut second, _) = second.unwrap();
    assert!(!root.path().join("startup.lock").exists());
    assert_eq!(
        requests(&root)
            .iter()
            .filter(|request| request["method"] == "initialize")
            .count(),
        2
    );
    first.close().await.unwrap();
    second.close().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn waiting_for_native_startup_is_bounded_and_rejects_before_spawning_acp() {
    let (root, mut client, spec) = fixture("normal");
    let startup_timeout = client.startup_timeout;
    client.startup_timeout = Duration::from_millis(150);
    let gate = client.startup.lock().await;
    assert!(matches!(
        launcher::spawn(&client, &spec.cwd, &StoredAgentConfig::default()).await,
        Err(AgentSessionError::Failed)
    ));
    assert!(!root.path().join("launch.jsonl").exists());
    drop(gate);
    // Only the blocked gate has a short budget; recovery must allow ordinary process startup.
    client.startup_timeout = startup_timeout;
    let (mut transport, _) = launcher::spawn(&client, &spec.cwd, &StoredAgentConfig::default())
        .await
        .unwrap();
    assert_eq!(transport.control_deadline(), client.deadline);
    transport.close().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn failed_native_initialization_releases_the_startup_gate() {
    let (_root, mut client, spec) = fixture("malformed");
    let selected = StoredAgentConfig::default();
    assert!(matches!(
        launcher::spawn(&client, &spec.cwd, &selected).await,
        Err(AgentSessionError::Failed)
    ));
    client
        .environment
        .insert("AIT_ACP_SCENARIO".into(), "normal".into());
    let (mut transport, _) = launcher::spawn(&client, &spec.cwd, &selected)
        .await
        .unwrap();
    transport.close().await.unwrap();
}
