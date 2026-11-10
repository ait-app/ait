use super::*;
use crate::service::agent_manager::{AgentManager, AgentRegistration};
use crate::storage::timeline::Timeline;
use domain::agent_runtime::AgentPersistenceHandle;
use domain::agent_runtime::registry::AgentRuntimeRegistry;
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; offline model protocol fixture, no paid inference"]
async fn installed_omp_btw_follow_up_and_branch_preserve_native_history_and_reopen() {
    let root = tempfile::tempdir().unwrap();
    let native = root.path().join("agent");
    std::fs::create_dir_all(native.join("extensions")).unwrap();
    let (url, _model) = model_roles::model_stub().await;
    model_roles::native_profile(&native, &url);
    let client = actions::installed_client(&native);
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
        .send("agent", "Keep the parent conversation")
        .await
        .unwrap();
    wait_for_idle(&mut manager).await;
    let original = registry.get("agent").unwrap().unwrap().persistence.unwrap();
    assert_follow_up_stays_in_native_topic(&mut manager).await;
    assert_eq!(
        registry
            .get("agent")
            .unwrap()
            .unwrap()
            .persistence
            .unwrap()
            .session_id,
        original.session_id
    );
    assert_branch_rejects_busy_or_changed_parent(&mut manager).await;
    let branch = assert_branch_preserves_parent_and_promotes_answer(
        &mut manager,
        &registry,
        &timeline,
        &original,
    )
    .await;
    manager.close_all().await.unwrap();
    let mut manager = AgentManager::new(Box::new(registry.clone())).with_timeline(timeline.clone());
    manager.register_client(Box::new(client)).unwrap();
    assert_reopened_branch_can_continue(&mut manager, &registry, &branch).await;
    manager.close_all().await.unwrap();
}

async fn wait_for_idle(manager: &mut AgentManager) {
    tokio::time::timeout(Duration::from_secs(30), async {
        while manager.active_turn("agent").is_some() {
            manager.poll().await.unwrap();
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

async fn assert_follow_up_stays_in_native_topic(manager: &mut AgentManager) {
    run(manager, "btw", json!({"question":"First BTW question"})).await;
    let first = wait_answer(manager).await;
    run(
        manager,
        "btw",
        json!({"question":"Follow up on this BTW topic", "recordId":first["id"]}),
    )
    .await;
    let followed = wait_answer(manager).await;
    assert_eq!(followed["id"], first["id"]);
    assert_eq!(followed["followUps"].as_array().unwrap().len(), 1);
    assert!(
        manager
            .begin_action("agent", "btw_branch", &json!({"recordId":first["id"]}))
            .await
            .is_err()
    );
}

async fn assert_branch_rejects_busy_or_changed_parent(manager: &mut AgentManager) {
    run(
        manager,
        "btw",
        json!({"question":"Promote this independent question"}),
    )
    .await;
    let promotable = wait_answer(manager).await;
    manager
        .send("agent", "Advance the parent after BTW")
        .await
        .unwrap();
    assert!(
        manager
            .begin_action("agent", "btw_branch", &json!({"recordId":promotable["id"]}))
            .await
            .is_err()
    );
    wait_for_idle(manager).await;
    assert!(
        manager
            .begin_action("agent", "btw_branch", &json!({"recordId":promotable["id"]}))
            .await
            .is_err()
    );
}

async fn assert_branch_preserves_parent_and_promotes_answer(
    manager: &mut AgentManager,
    registry: &FileBackedAgentRuntimeRegistry,
    timeline: &Timeline,
    original: &AgentPersistenceHandle,
) -> AgentPersistenceHandle {
    run(
        manager,
        "btw",
        json!({"question":"Promote this independent question"}),
    )
    .await;
    let promotable = wait_answer(manager).await;
    let parent_file = original.metadata.as_ref().unwrap()["sessionFile"]
        .as_str()
        .unwrap();
    let original_bytes = std::fs::read(parent_file).unwrap();
    let result = run(manager, "btw_branch", json!({"recordId":promotable["id"]})).await;
    assert!(result["error"].is_null(), "{result}");
    let branch = registry.get("agent").unwrap().unwrap().persistence.unwrap();
    assert_ne!(branch.session_id, original.session_id);
    assert_eq!(std::fs::read(parent_file).unwrap(), original_bytes);
    let contents = std::fs::read_to_string(
        branch.metadata.as_ref().unwrap()["sessionFile"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let header = contents
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|entry| entry["type"] == "session")
        .unwrap();
    assert_eq!(
        header["parentSession"], parent_file,
        "header={header}, branch={branch:?}"
    );
    let rows = timeline.read("agent").unwrap().1;
    assert!(
        rows.iter()
            .any(|row| row.entry.item["text"] == "Keep the parent conversation")
    );
    assert!(
        rows.iter()
            .any(|row| row.entry.item["text"] == "Promote this independent question")
    );
    assert!(
        !rows
            .iter()
            .any(|row| row.entry.item["text"] == "First BTW question")
    );
    branch
}

async fn assert_reopened_branch_can_continue(
    manager: &mut AgentManager,
    registry: &FileBackedAgentRuntimeRegistry,
    branch: &AgentPersistenceHandle,
) {
    run(manager, "get_session_stats", json!({})).await;
    assert_eq!(
        registry
            .get("agent")
            .unwrap()
            .unwrap()
            .persistence
            .unwrap()
            .session_id,
        branch.session_id
    );
    let messages = run(manager, "get_entries", json!({})).await;
    assert!(
        messages["text"]
            .as_str()
            .unwrap()
            .contains("Promote this independent question")
    );
    manager
        .send("agent", "Continue working in the BTW branch")
        .await
        .unwrap();
    wait_for_idle(manager).await;
}

async fn run(manager: &mut AgentManager, action: &str, arguments: Value) -> Value {
    let mut receipt = manager
        .begin_action("agent", action, &arguments)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            manager.poll().await.unwrap();
            tokio::select! {
                result = &mut receipt => { let result = result.unwrap(); assert!(result["error"].is_null(), "{result}"); break result; },
                () = tokio::time::sleep(Duration::from_millis(10)) => {}
            }
        }
    }).await.unwrap()
}

async fn wait_answer(manager: &mut AgentManager) -> Value {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let data = run(manager, "get_btw_history", json!({})).await;
            let data: Value = serde_json::from_str(data["text"].as_str().unwrap()).unwrap();
            let record = &data["records"][0];
            let latest = record["followUps"]
                .as_array()
                .and_then(|turns| turns.last())
                .unwrap_or(record);
            if latest["status"] == "complete" {
                break record.clone();
            }
            assert_eq!(latest["status"], "running", "{latest}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap()
}
