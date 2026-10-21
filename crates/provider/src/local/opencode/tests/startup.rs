//! Startup regressions use protocol barriers instead of real network or model latency.
use super::*;

#[cfg(unix)]
#[tokio::test]
async fn availability_does_not_boot_acp_or_load_the_daemons_project() {
    let (root, client, spec) = fixture("hung");

    assert!(client.is_available().await.unwrap());

    assert!(!root.path().join("launch.jsonl").exists());
    assert!(!root.path().join("native-fixture.json").exists());
    assert_eq!(
        client.settings(&spec.config)["capabilities"]["supportsSessionListing"],
        false
    );
}

#[cfg(unix)]
#[tokio::test]
async fn discovery_initializes_acp_once_and_records_native_capabilities() {
    let (root, client, spec) = fixture("normal");

    assert!(client.is_available().await.unwrap());
    let details = client.discover(&spec.cwd).await.unwrap();

    assert_eq!(details.models.len(), 2);
    assert_eq!(
        requests(&root)
            .iter()
            .filter(|request| request["method"] == "initialize")
            .count(),
        1
    );
    assert_eq!(
        client.settings(&spec.config)["capabilities"]["supportsSessionListing"],
        true
    );
    assert!(!root.path().join("native-fixture.json").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn legacy_catalog_reads_models_while_primary_modes_are_loading() {
    let (root, mut client, spec) = fixture("catalog-parallel");
    client
        .environment
        .insert("AIT_ACP_VERSION".into(), "1.17.0".into());

    let details = client.discover(&spec.cwd).await.unwrap();

    assert_eq!(details.models.len(), 2);
    assert_eq!(details.modes.len(), 3);
    assert!(root.path().join("models.entered").exists());
    assert!(root.path().join("agents.entered").exists());
    assert!(!root.path().join("native-fixture.json").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn cold_history_restore_uses_startup_budget() {
    let (root, mut client, spec) = fixture("normal");
    let mut session = client.create_session(&spec).await.unwrap();
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    client
        .environment
        .insert("AIT_ACP_SCENARIO".into(), "slow-load".into());
    tokio::time::pause();
    let pending = tokio::spawn(async move {
        let mut session = client
            .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
            .await?;
        session.close().await
    });

    compatibility::release_after_old_deadline(&root, "load").await;

    pending.await.unwrap().unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn cold_configuration_reconnect_uses_startup_budget() {
    for scenario in ["slow-resume", "slow-load-only"] {
        let (root, client, mut spec) = fixture(scenario);
        let mut session = client.create_session(&spec).await.unwrap();
        spec.config.system_prompt = Some("Reconnected system prompt".into());
        tokio::time::pause();
        let pending = tokio::spawn(async move {
            session.start_turn("reconnect", &spec.config).await?;
            completed(session.as_mut()).await;
            session.close().await
        });

        compatibility::release_after_old_deadline(&root, "reconnect").await;

        pending.await.unwrap().unwrap();
    }
}

#[cfg(unix)]
#[tokio::test(start_paused = true)]
async fn auxiliary_session_creation_uses_the_same_startup_budget() {
    let (root, client, spec) = fixture("slow-session");
    let pending = tokio::spawn(async move {
        client
            .generate_summary(&spec, "metadata", &json!({"type":"object"}))
            .await
    });

    compatibility::release_after_old_deadline(&root, "session").await;

    assert_eq!(pending.await.unwrap().unwrap(), "authoritative answer");
    assert!(!root.path().join("native-fixture.json").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn legacy_owned_catalog_avoids_extra_cli_processes_and_native_sessions() {
    let (root, mut client, spec) = fixture("normal");
    client
        .environment
        .insert("AIT_ACP_VERSION".into(), "1.18.4".into());

    assert!(client.is_available().await.unwrap());
    let details = client.discover(&spec.cwd).await.unwrap();

    assert_eq!(details.models.len(), 2);
    assert_eq!(
        details.models[0]["thinkingOptions"],
        json!([{"id":"high","label":"high"}])
    );
    assert_eq!(details.modes.len(), 3);
    assert!(!root.path().join("native-fixture.json").exists());
    assert_eq!(
        std::fs::read_to_string(root.path().join("pids.txt"))
            .unwrap()
            .lines()
            .count(),
        3
    );
    let requests: Vec<Value> = std::fs::read_to_string(root.path().join("http-requests.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        requests,
        vec![
            json!({"method":"GET","path":"/config/providers"}),
            json!({"method":"GET","path":"/agent"})
        ]
    );
}

#[cfg(unix)]
#[tokio::test]
async fn legacy_owned_catalog_failure_falls_back_to_cli_without_creating_a_session() {
    let (root, mut client, spec) = fixture("catalog-http-failed");
    client
        .environment
        .insert("AIT_ACP_VERSION".into(), "1.18.4".into());

    let details = client.discover(&spec.cwd).await.unwrap();

    assert_eq!(details.models.len(), 2);
    assert_eq!(details.modes.len(), 3);
    assert!(!root.path().join("native-fixture.json").exists());
    assert!(
        std::fs::read_to_string(root.path().join("pids.txt"))
            .unwrap()
            .lines()
            .count()
            > 3
    );
}
