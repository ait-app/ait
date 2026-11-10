use super::*;
use crate::service::agent_manager::{AgentManager, AgentRegistration};
use crate::storage::timeline::Timeline;
use domain::agent_runtime::AgentPersistenceHandle;
use domain::agent_runtime::registry::AgentRuntimeRegistry;
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; uses only an offline model protocol fixture"]
async fn installed_omp_native_actions_commit_forks_tree_navigation_and_reopen() {
    let root = tempfile::tempdir().unwrap();
    let native = root.path().join("agent");
    std::fs::create_dir_all(native.join("extensions")).unwrap();
    let (url, _model) = model_roles::model_stub().await;
    model_roles::native_profile(&native, &url);
    let client = installed_client(&native);
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    let timeline = Timeline::open(&root.path().join("timeline.sqlite")).unwrap();
    let mut manager = AgentManager::new(Box::new(registry.clone())).with_timeline(timeline.clone());
    manager.register_client(Box::new(client.clone())).unwrap();
    let spec = AgentSessionSpec {
        confinement: None,
        provider: "omp".to_owned(),
        cwd: root.path().to_str().unwrap().to_owned(),
        config: StoredAgentConfig {
            model: Some(json!(["ait-role-fixture", "main"]).to_string()),
            ..StoredAgentConfig::default()
        },
    };
    manager
        .create("agent", &spec, AgentRegistration::default())
        .await
        .unwrap();
    manager
        .send("agent", "Create the persistent native fixture")
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(60), async {
        while manager.active_turn("agent").is_some() {
            manager.poll().await.unwrap();
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_native_goal_controls(&mut manager).await;
    let original = registry.get("agent").unwrap().unwrap().persistence.unwrap();
    run(&mut manager, "fork", json!({})).await;
    let forked = registry.get("agent").unwrap().unwrap();
    assert_ne!(
        forked.persistence.as_ref().unwrap().session_id,
        original.session_id
    );
    assert_eq!(
        forked.runtime_info.unwrap().session_id,
        Some(forked.persistence.as_ref().unwrap().session_id.clone())
    );
    assert!(
        timeline
            .read("agent")
            .unwrap()
            .1
            .iter()
            .any(|row| row.entry.item["text"] == "Role fixture ready")
    );
    let entries = run(&mut manager, "get_entries", json!({})).await;
    let entries: Value = serde_json::from_str(entries["text"].as_str().unwrap()).unwrap();
    let target = entries["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["message"]["role"] == "user")
        .unwrap()["id"]
        .clone();
    run(&mut manager, "navigate_tree", json!({"entryId":target})).await;
    assert!(
        !timeline
            .read("agent")
            .unwrap()
            .1
            .iter()
            .any(|row| row.entry.item["text"] == "Role fixture ready")
    );
    let navigated = registry.get("agent").unwrap().unwrap().persistence.unwrap();
    manager.close_all().await.unwrap();
    let mut manager = AgentManager::new(Box::new(registry.clone())).with_timeline(timeline.clone());
    manager.register_client(Box::new(client)).unwrap();
    assert_restored_navigation(&mut manager, &timeline, &navigated).await;
    assert_eq!(
        registry
            .get("agent")
            .unwrap()
            .unwrap()
            .persistence
            .unwrap()
            .session_id,
        navigated.session_id
    );
    run(&mut manager, "new_session", json!({})).await;
    assert!(timeline.read("agent").unwrap().1.is_empty());
    manager.close_all().await.unwrap();
}

async fn assert_native_goal_controls(manager: &mut AgentManager) {
    let initial = run(manager, "goal", json!({"op":"get"})).await;
    let initial: Value = serde_json::from_str(initial["text"].as_str().unwrap()).unwrap();
    assert!(initial["goal"].is_null());
    for (arguments, status) in [
        (
            json!({"op":"create","objective":"Offline goal fixture","token_budget":"100"}),
            "active",
        ),
        (json!({"op":"pause"}), "paused"),
        (json!({"op":"resume"}), "active"),
    ] {
        let result = run(manager, "goal", arguments).await;
        let result: Value = serde_json::from_str(result["text"].as_str().unwrap()).unwrap();
        assert_eq!(result["goal"]["status"], status);
        assert_eq!(result["goal"]["objective"], "Offline goal fixture");
    }
    run(manager, "goal", json!({"op":"drop"})).await;
    let dropped = run(manager, "goal", json!({"op":"get"})).await;
    let dropped: Value = serde_json::from_str(dropped["text"].as_str().unwrap()).unwrap();
    assert!(dropped["state"].is_null());
}

async fn assert_restored_navigation(
    manager: &mut AgentManager,
    timeline: &Timeline,
    handle: &AgentPersistenceHandle,
) {
    run(manager, "get_session_stats", json!({})).await;
    let tree = run(manager, "get_tree", json!({})).await;
    let tree: Value = serde_json::from_str(tree["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        tree["leafId"],
        handle.metadata.as_ref().unwrap()["navigation"]["leafId"],
        "checkpoint: {handle:?}"
    );
    assert!(
        !timeline
            .read("agent")
            .unwrap()
            .1
            .iter()
            .any(|row| row.entry.item["text"] == "Role fixture ready")
    );
}

/// Build an isolated external OMP client for native session integration checks.
pub(super) fn installed_client(native: &std::path::Path) -> OmpClient {
    OmpClient {
        environment: AgentEnvironment::try_from(BTreeMap::from([
            (
                "PI_CODING_AGENT_DIR".to_owned(),
                native.to_str().unwrap().to_owned(),
            ),
            ("OMP_PROFILE".to_owned(), String::new()),
            ("PI_PROFILE".to_owned(), String::new()),
        ]))
        .unwrap(),
        ..OmpClient::new(
            std::env::var_os("AIT_TEST_OMP_BIN")
                .expect("set AIT_TEST_OMP_BIN")
                .into(),
        )
    }
}

async fn run(manager: &mut AgentManager, action: &str, arguments: Value) -> Value {
    let mut receipt = manager
        .begin_action("agent", action, &arguments)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            manager.poll().await.unwrap();
            tokio::select! {
                result = &mut receipt => {
                    let result = result.unwrap();
                    assert!(result["error"].is_null(),"{result}");
                    break result;
                }
                () = tokio::time::sleep(Duration::from_millis(10)) => {}
            }
        }
    })
    .await
    .unwrap()
}
