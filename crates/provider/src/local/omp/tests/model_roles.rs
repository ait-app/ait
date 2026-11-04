use super::*;

fn set_role(config: &mut StoredAgentConfig, role: &str, selector: &str) {
    config
        .feature_values
        .get_or_insert_default()
        .insert(format!("modelRole.{role}"), json!(selector));
}

fn launches(root: &std::path::Path) -> Vec<Value> {
    std::fs::read_to_string(root.join("agent/calls.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|call| {
            call.get("args").is_some()
                && call["args"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|arg| arg != "--no-session")
        })
        .collect()
}

#[tokio::test]
async fn known_role_selections_validate_without_starting_another_native_process() {
    let (root, client, mut spec) = fixture();
    let details = client.discover(&spec.cwd).await.unwrap();
    let log = root.path().join("agent/calls.jsonl");
    let previous = std::fs::read_to_string(&log).unwrap();
    assert_eq!(
        client.draft_features(&spec).await.unwrap(),
        details.features
    );
    set_role(&mut spec.config, "advisor", "other/model:high");
    client.validate_selection(&spec).await.unwrap();
    set_role(&mut spec.config, "advisor", "unknown/model");
    assert_eq!(
        client.validate_selection(&spec).await.unwrap_err(),
        AgentSessionError::Rejected
    );
    set_role(&mut spec.config, "advisor", "native");
    client.validate_selection(&spec).await.unwrap();
    assert_eq!(std::fs::read_to_string(log).unwrap(), previous);
    let mut other = client.clone();
    let mut environment: BTreeMap<String, String> = client
        .environment
        .entries()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    environment.insert("OMP_PROFILE".to_owned(), "another-profile".to_owned());
    other.environment = AgentEnvironment::try_from(environment).unwrap();
    set_role(&mut spec.config, "advisor", "other/model:high");
    other.validate_selection(&spec).await.unwrap();
    assert!(
        std::fs::read_to_string(root.path().join("agent/calls.jsonl"))
            .unwrap()
            .len()
            > previous.len()
    );
}

#[tokio::test]
async fn concurrent_discovery_shares_one_native_bootstrap_and_later_refresh_still_reloads() {
    let (root, client, spec) = fixture();
    let (first, second) = tokio::join!(client.discover(&spec.cwd), client.discover(&spec.cwd));
    assert_eq!(first.unwrap().models, second.unwrap().models);
    let metadata_reads = || {
        std::fs::read_to_string(root.path().join("agent/metadata-calls.jsonl"))
            .unwrap()
            .lines()
            .count()
    };
    assert_eq!(metadata_reads(), 2);
    let native_readers = || {
        std::fs::read_to_string(root.path().join("agent/calls.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .filter(|call| {
                call["args"]
                    .as_array()
                    .is_some_and(|args| args.iter().any(|arg| arg == "--no-session"))
            })
            .count()
    };
    assert_eq!(native_readers(), 1);
    client.discover(&spec.cwd).await.unwrap();
    assert_eq!(metadata_reads(), 4);
    assert_eq!(native_readers(), 2);
}

#[tokio::test]
async fn creating_a_session_reuses_retained_models_until_explicit_discovery() {
    let (root, client, spec) = fixture();
    client.discover(&spec.cwd).await.unwrap();
    for snapshot in client.role_catalogs.write().unwrap().values_mut() {
        snapshot.updated -= Duration::from_hours(24);
    }
    client.draft_features(&spec).await.unwrap();
    let catalog_reads = || {
        std::fs::read_to_string(root.path().join("agent/metadata-calls.jsonl"))
            .unwrap()
            .lines()
            .count()
    };
    let previous = catalog_reads();
    assert_eq!(previous, 2);
    let mut session = client.create_session(&spec).await.unwrap();
    session.close().await.unwrap();
    assert_eq!(catalog_reads(), previous);
    client.discover(&spec.cwd).await.unwrap();
    assert_eq!(catalog_reads(), previous + 2);
}

#[tokio::test]
async fn native_roles_and_custom_tags_are_discovered_without_creating_conversations() {
    let (root, client, spec) = fixture();
    let agent = root.path().join("agent");
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::write(
        agent.join("model-tags.json"),
        json!({"custom":{"name":"Project helper"}}).to_string(),
    )
    .unwrap();
    std::fs::write(
        agent.join("cycle-order.json"),
        json!(["cycle-only"]).to_string(),
    )
    .unwrap();
    let details = client.discover(&spec.cwd).await.unwrap();
    assert_eq!(
        details.models.len(),
        2,
        "non-chat kinds must not enter the main model picker"
    );
    let roles: Vec<_> = details
        .features
        .iter()
        .filter(|feature| feature["id"].as_str().unwrap().starts_with("modelRole."))
        .collect();
    assert_eq!(roles.len(), 18);
    let web = roles
        .iter()
        .find(|feature| feature["id"] == "modelRole.web")
        .unwrap();
    assert_eq!(
        web["options"]
            .as_array()
            .unwrap()
            .iter()
            .map(|option| option["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "native",
            "other/model",
            "other/model:low",
            "other/model:high",
            "other/model:max",
            "test/search"
        ]
    );
    assert!(
        roles
            .iter()
            .any(|feature| feature["label"] == "Project helper model (custom)")
    );
    assert!(!agent.join("sessions").exists());
    assert!(launches(root.path()).is_empty());
}

#[tokio::test]
async fn role_updates_reopen_one_persisted_owner_and_reset_to_native_without_mutating_defaults() {
    let (root, client, mut spec) = fixture();
    let agent = root.path().join("agent");
    std::fs::create_dir_all(&agent).unwrap();
    let original =
        json!({"default":"test/model","smol":"other/model","advisor":"other/model:high"})
            .to_string();
    std::fs::write(agent.join("model-roles.json"), &original).unwrap();
    set_role(&mut spec.config, "default", "other/model");
    set_role(&mut spec.config, "vision", "test/model");
    set_role(&mut spec.config, "image", "test/image");
    let mut session = client.create_session(&spec).await.unwrap();
    assert_eq!(
        session.runtime_info().await.unwrap().model,
        Some(json!(["other", "model"]).to_string())
    );
    session.start_turn("first", &spec.config).await.unwrap();
    complete(session.as_mut()).await;
    let handle = session.persistence().unwrap();
    set_role(&mut spec.config, "advisor", "test/model");
    assert!(session.validate_config_update(&spec.config).is_ok());
    session.start_turn("second", &spec.config).await.unwrap();
    complete(session.as_mut()).await;
    assert_eq!(session.persistence().unwrap().session_id, handle.session_id);
    let calls = launches(root.path());
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1]["modelRoles"]["smol"], "other/model");
    assert_eq!(calls[1]["modelRoles"]["advisor"], "test/model");
    let overlay_path = calls[1]["args"]
        .as_array()
        .unwrap()
        .windows(2)
        .find(|args| args[0] == "--config")
        .unwrap()[1]
        .as_str()
        .unwrap();
    assert!(std::path::Path::new(overlay_path).is_file());
    for role in ["default", "vision", "image", "advisor"] {
        set_role(&mut spec.config, role, "native");
    }
    session.start_turn("third", &spec.config).await.unwrap();
    complete(session.as_mut()).await;
    assert_eq!(session.persistence().unwrap().session_id, handle.session_id);
    assert!(!std::path::Path::new(overlay_path).exists());
    let calls = launches(root.path());
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[2]["modelRoles"]["advisor"], "other/model:high");
    assert!(
        !calls[2]["args"]
            .as_array()
            .unwrap()
            .contains(&json!("--config"))
    );
    session.close().await.unwrap();
    let history = client.history(&handle, &spec.cwd).await.unwrap();
    for text in ["first", "second", "third"] {
        assert!(history.iter().any(|entry| entry.item["text"] == text));
    }
    assert_eq!(
        std::fs::read_to_string(agent.join("model-roles.json")).unwrap(),
        original
    );
}

#[tokio::test]
async fn invalid_roles_reject_before_native_launch_and_leave_the_current_owner_usable() {
    let (root, client, mut spec) = fixture();
    for (role, model) in [
        ("not-configured", "test/model"),
        ("image", "test/model"),
        ("smol", "missing/model"),
    ] {
        set_role(&mut spec.config, role, model);
        assert!(client.create_session(&spec).await.is_err());
        spec.config.feature_values = None;
    }
    assert!(launches(root.path()).is_empty());
    let mut session = client.create_session(&spec).await.unwrap();
    set_role(&mut spec.config, "speech", "test/model");
    assert!(session.validate_config_update(&spec.config).is_err());
    assert!(
        session
            .start_turn("not-admitted", &spec.config)
            .await
            .is_err()
    );
    spec.config.feature_values = None;
    session.start_turn("valid", &spec.config).await.unwrap();
    complete(session.as_mut()).await;
    assert_eq!(launches(root.path()).len(), 1);
    session.close().await.unwrap();
}

#[tokio::test]
async fn role_restart_waits_for_native_background_settlement() {
    let (root, client, mut spec) = fixture();
    let mut session = session::open(&client, &spec, None).await.unwrap();
    session
        .start_turn("busy-after-yield", &spec.config)
        .await
        .unwrap();
    assert_eq!(
        session
            .native_request("get_state", json!({}))
            .await
            .unwrap()["hasPendingAsyncWork"],
        true
    );
    set_role(&mut spec.config, "smol", "other/model");
    assert!(session.configure(&spec.config).await.is_err());
    std::fs::write(root.path().join("agent/release-settlement"), "").unwrap();
    assert_eq!(
        complete(&mut session).await.last(),
        Some(&AgentTurnEvent::Completed(Some(
            "Background finished".to_owned()
        )))
    );
    session.configure(&spec.config).await.unwrap();
    session.close().await.unwrap();
}

#[tokio::test]
async fn role_changes_require_explicit_idle_facts_before_replacing_the_owner() {
    let (root, client, mut spec) = fixture();
    let mut session = session::open(&client, &spec, None).await.unwrap();
    set_role(&mut spec.config, "smol", "other/model");
    for flag in ["idle-busy", "idle-unknown"] {
        let file = root.path().join("agent").join(flag);
        std::fs::write(&file, "").unwrap();
        assert_eq!(
            session.configure(&spec.config).await,
            Err(AgentSessionError::Busy)
        );
        assert_eq!(launches(root.path()).len(), 1);
        std::fs::remove_file(file).unwrap();
    }
    session.configure(&spec.config).await.unwrap();
    assert_eq!(launches(root.path()).len(), 2);
    session.close().await.unwrap();
}

#[tokio::test]
async fn manager_persists_role_selections_across_restart_and_replays_the_same_session() {
    use crate::service::agent_manager::{AgentManager, AgentRegistration};
    use domain::agent_runtime::registry::AgentRuntimeRegistry;
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
    let (root, client, spec) = fixture();
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    let mut manager = AgentManager::new(Box::new(registry.clone()));
    manager.register_client(Box::new(client.clone())).unwrap();
    manager
        .create("roles", &spec, AgentRegistration::default())
        .await
        .unwrap();
    let patch = serde_json::from_value(
        json!({"featureValues":{"modelRole.smol":"other/model","modelRole.advisor":"test/model"}}),
    )
    .unwrap();
    manager.configure("roles", &patch).await.unwrap();
    manager.send("roles", "before restart").await.unwrap();
    while manager.active_turn("roles").is_some() {
        manager.poll().await.unwrap();
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    let record = registry.get("roles").unwrap().unwrap();
    let id = record.persistence.as_ref().unwrap().session_id.clone();
    assert_eq!(
        record.config.unwrap().feature_values.unwrap()["modelRole.advisor"],
        "test/model"
    );
    manager.close_all().await.unwrap();
    let mut manager = AgentManager::new(Box::new(registry.clone()));
    manager.register_client(Box::new(client)).unwrap();
    manager.resume("roles").await.unwrap();
    assert_eq!(
        registry
            .get("roles")
            .unwrap()
            .unwrap()
            .persistence
            .as_ref()
            .unwrap()
            .session_id,
        id
    );
    let calls = launches(root.path());
    assert_eq!(calls.last().unwrap()["modelRoles"]["smol"], "other/model");
    assert_eq!(calls.last().unwrap()["modelRoles"]["advisor"], "test/model");
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn live_session_accepts_new_role_and_model_after_background_catalog_refresh() {
    use crate::service::agent_manager::{AgentManager, AgentRegistration};
    use domain::agent_runtime::registry::AgentRuntimeRegistry;
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
    let (root, client, spec) = fixture();
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    let mut manager = AgentManager::new(Box::new(registry.clone()));
    manager.register_client(Box::new(client.clone())).unwrap();
    manager
        .create("refresh", &spec, AgentRegistration::default())
        .await
        .unwrap();
    std::fs::write(root.path().join("agent/extra-models.json"), json!([{"selector":"future/model","name":"Added model","kind":"chat","input":["text"],"thinking":["high"]}]).to_string()).unwrap();
    std::fs::write(
        root.path().join("agent/model-tags.json"),
        json!({"future":{"name":"New helper"}}).to_string(),
    )
    .unwrap();
    client.discover(&spec.cwd).await.unwrap();
    let patch = serde_json::from_value(json!({"featureValues":{"modelRole.future":"future/model","modelRole.advisor":"future/model:high"}})).unwrap();
    manager.configure("refresh", &patch).await.unwrap();
    client.role_catalogs.write().unwrap().clear();
    let patch =
        serde_json::from_value(json!({"featureValues":{"modelRole.future":"future/model:high"}}))
            .unwrap();
    manager.configure("refresh", &patch).await.unwrap();
    let record = registry.get("refresh").unwrap().unwrap();
    let mut snapshot = json!({});
    manager.control_snapshot(&record, &mut snapshot);
    assert!(
        snapshot["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|feature| feature["id"] == "modelRole.future"
                && feature["options"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|option| option["id"] == "future/model"))
    );
    assert_eq!(
        registry
            .get("refresh")
            .unwrap()
            .unwrap()
            .config
            .unwrap()
            .feature_values
            .unwrap()["modelRole.future"],
        "future/model:high"
    );
    manager
        .send("refresh", "Use the refreshed role")
        .await
        .unwrap();
    while manager.active_turn("refresh").is_some() {
        manager.poll().await.unwrap();
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    assert_eq!(
        launches(root.path()).last().unwrap()["modelRoles"]["future"],
        "future/model:high"
    );
    manager.close_all().await.unwrap();
}
