use super::*;
use crate::ports::agent_session::AgentClient;

#[tokio::test]
async fn startup_dialogs_publish_before_admission_and_do_not_replay_a_cancelled_input() {
    let (root, client, spec) = crate::local::omp::tests::fixture();
    std::fs::create_dir_all(root.path().join("agent")).unwrap();
    std::fs::write(root.path().join("agent/startup-dialog"), "true").unwrap();
    let mut session = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client.create_session(&spec),
    )
    .await
    .expect("startup dialog must not block session publication")
    .unwrap();
    let request = session
        .pending_permissions()
        .into_iter()
        .next()
        .expect("startup request");
    let turn = session
        .start_turn("must not be replayed", &spec.config)
        .await
        .unwrap();
    session.cancel_turn(&turn).await.unwrap();
    assert!(session.pending_permissions().is_empty());
    assert!(session.start_turn("later", &spec.config).await.is_err());
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(!calls.contains("must not be replayed"));
    assert!(request["id"].as_str().is_some());
}

#[tokio::test]
async fn startup_empty_answer_and_cancel_remain_distinct_and_release_one_pending_prompt() {
    for answer in [
        serde_json::json!({"behavior":"allow","updatedInput":{"answers":{"value":""}}}),
        serde_json::json!({"behavior":"deny"}),
    ] {
        let (root, client, spec) = crate::local::omp::tests::fixture();
        std::fs::create_dir_all(root.path().join("agent")).unwrap();
        std::fs::write(root.path().join("agent/startup-dialog"), "true").unwrap();
        let mut session = client.create_session(&spec).await.unwrap();
        let request = session.pending_permissions().pop().unwrap();
        session.start_turn("Only once", &spec.config).await.unwrap();
        session
            .respond_permission(request["id"].as_str().unwrap(), &answer)
            .await
            .unwrap();
        let events = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let mut events = Vec::new();
            loop {
                session.service_realtime().await.unwrap();
                while let Some(event) = session.poll_turn().unwrap() {
                    let done = matches!(event, AgentTurnEvent::Completed(_));
                    events.push(event);
                    if done {
                        return events;
                    }
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(session.pending_permissions().is_empty());
        assert!(
            events.iter().any(|event| matches!(event,
            AgentTurnEvent::Timeline(entry) if entry.item["text"] == "Only once")),
            "{events:?}"
        );
        let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
        let calls: Vec<Value> = calls
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let replies: Vec<_> = calls
            .iter()
            .filter(|call| call["type"] == "extension_ui_response")
            .collect();
        assert_eq!(replies.len(), 1);
        if answer["behavior"] == "allow" {
            assert_eq!(replies[0]["value"], "");
            assert!(replies[0].get("cancelled").is_none());
        } else {
            assert_eq!(replies[0]["cancelled"], true);
            assert!(replies[0].get("value").is_none());
        }
        assert_eq!(
            calls.iter().filter(|call| call["type"] == "prompt").count(),
            1
        );
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn manager_publishes_a_startup_question_before_the_first_input_is_admitted() {
    use crate::service::agent_manager::{AgentManager, AgentRegistration};
    use crate::storage::timeline::Timeline;
    let (root, client, spec) = crate::local::omp::tests::fixture();
    std::fs::create_dir_all(root.path().join("agent")).unwrap();
    std::fs::write(root.path().join("agent/startup-dialog"), "true").unwrap();
    let registry = persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry::new(
        root.path().join("agents.json"),
    );
    let timeline = Timeline::open(&root.path().join("timeline.sqlite")).unwrap();
    let mut manager = AgentManager::new(Box::new(registry)).with_timeline(timeline.clone());
    manager.register_client(Box::new(client)).unwrap();
    let record = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        manager.create("startup-agent", &spec, AgentRegistration::default()),
    )
    .await
    .unwrap()
    .unwrap();
    let mut snapshot = serde_json::json!({});
    manager.control_snapshot(&record, &mut snapshot);
    assert_eq!(snapshot["pendingPermissions"][0]["id"], "startup-choice");
    assert!(record.persistence.is_none());
    manager
        .send("startup-agent", "Only after the user answers")
        .await
        .unwrap();
    manager
        .permission(
            "startup-agent",
            "startup-choice",
            &serde_json::json!({"behavior":"deny"}),
        )
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while manager.active_turn("startup-agent").is_some() {
            manager.poll().await.unwrap();
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(
        manager
            .live_snapshot("startup-agent")
            .unwrap()
            .persistence
            .is_some()
    );
    let (_, entries) = timeline.read("startup-agent").unwrap();
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.entry.item["text"] == "Only after the user answers")
            .count(),
        1
    );
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn metadata_reads_keep_project_commands_and_do_not_request_startup_dialogs() {
    let (root, client, spec) = crate::local::omp::tests::fixture();
    std::fs::create_dir_all(root.path().join("agent")).unwrap();
    std::fs::write(root.path().join("agent/startup-dialog"), "true").unwrap();
    std::fs::write(
        root.path().join("agent/plugins.json"),
        serde_json::json!([{"name":"record","source":"extension","description":"Project time tracking"}])
            .to_string(),
    )
    .unwrap();
    let commands = tokio::time::timeout(std::time::Duration::from_secs(5), client.commands(&spec))
        .await
        .unwrap()
        .unwrap();
    assert!(
        commands
            .iter()
            .any(|command| command["name"] == "record" && command["kind"] == "command")
    );
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(!calls.contains("extension_ui_response"));
    assert!(!calls.contains("\"type\": \"prompt\""));
    assert!(!calls.contains("--no-extensions"));
    assert!(calls.contains("--no-ui"));
}
