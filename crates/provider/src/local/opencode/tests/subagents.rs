use super::*;
use provider_core::ports::controls::SubagentEvent;

#[tokio::test]
async fn timed_out_child_discovery_reaps_its_native_process_before_returning() {
    let (root, mut client, spec) = fixture("subagents-list-timeout");
    let timeout = client.startup_timeout;
    client.deadline = timeout + Duration::from_secs(1);
    let pending = tokio::spawn(async move { client.subagents(&spec.cwd).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !root.path().join("list.entered").exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();

    tokio::time::pause();
    tokio::time::advance(timeout).await;
    tokio::time::resume();
    assert_eq!(pending.await.unwrap(), Err(AgentSessionError::Failed));

    for pid in std::fs::read_to_string(root.path().join("pids.txt"))
        .unwrap()
        .lines()
    {
        let status = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", pid])
            .output()
            .unwrap();
        assert!(status.status.success() || status.status.code() == Some(1));
        assert!(
            String::from_utf8(status.stdout).unwrap().trim().is_empty(),
            "discovery returned before reaping native process {pid}"
        );
    }
}

#[tokio::test]
async fn opencode_subagents_are_persisted_and_queryable_after_host_restart() {
    use crate::service::agent_manager::{AgentManager, AgentRegistration};
    use domain::agent_runtime::registry::AgentRuntimeRegistry;
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

    let (root, client, spec) = fixture("subagents");
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    registry.initialize().unwrap();
    let timeline = crate::storage::timeline::Timeline::memory().unwrap();
    let mut manager = AgentManager::new(Box::new(registry.clone())).with_timeline(timeline.clone());
    manager.register_client(Box::new(client.clone())).unwrap();
    manager
        .create("parent", &spec, AgentRegistration::default())
        .await
        .unwrap();
    manager.send("parent", "Explore").await.unwrap();
    poll_until_idle(&mut manager, "parent").await;
    let requests_before_listing = requests(&root).len();
    assert_eq!(manager.subagents("parent").await.unwrap().len(), 2);
    assert_eq!(requests(&root).len(), requests_before_listing);
    assert_eq!(timeline.subagents("parent").unwrap().len(), 2);
    let (_, rows) = timeline.read("subagent:parent:ses_nested").unwrap();
    assert!(
        rows.iter()
            .any(|row| row.entry.item["text"] == "CHILD ses_nested")
    );
    manager.close_all().await.unwrap();

    let mut restarted = AgentManager::new(Box::new(registry)).with_timeline(timeline);
    restarted.register_client(Box::new(client)).unwrap();
    let children = restarted.subagents("parent").await.unwrap();
    assert_eq!(children.len(), 2);
    for child in children {
        let history = restarted.child_history("parent", &child).await.unwrap();
        assert_eq!(history.parent_id.as_deref(), Some(child.parent_id.as_str()));
    }
}

#[tokio::test]
async fn acp_background_children_keep_streaming_and_answering_permissions_after_parent_completion()
{
    let (root, client, spec) = fixture("subagents-idle");
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("Background", &spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    assert_eq!(session.subagents()[0].descriptor["status"], "running");
    let mut answered = false;
    let mut output = false;
    loop {
        match event(session.as_mut()).await {
            AgentTurnEvent::PermissionRequested(request) => {
                assert!(!answered);
                session
                    .respond_permission(
                        request["id"].as_str().unwrap(),
                        &json!({"behavior":"allow","selectedActionId":"once"}),
                    )
                    .await
                    .unwrap();
                answered = true;
            }
            AgentTurnEvent::Subagent(SubagentEvent::Progress { entry, .. }) => {
                output |= entry.item["text"] == "Background answer";
            }
            AgentTurnEvent::Subagent(SubagentEvent::Upsert(child))
                if child.descriptor["status"] == "completed" =>
            {
                break;
            }
            AgentTurnEvent::Failed | AgentTurnEvent::Cancelled | AgentTurnEvent::Completed(_) => {
                panic!("child changed parent outcome")
            }
            _ => {}
        }
    }
    assert!(answered && output);
    assert_eq!(
        requests(&root)
            .iter()
            .filter(|request| request["method"] == "session/prompt")
            .count(),
        1
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn acp_subagents_stream_nested_children_without_polluting_the_parent() {
    let (_root, client, spec) = fixture("subagents");
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("Explore", &spec.config).await.unwrap();
    let mut children = BTreeMap::new();
    let mut child_text = Vec::new();
    let mut parent_text = Vec::new();
    loop {
        match event(session.as_mut()).await {
            AgentTurnEvent::Subagent(SubagentEvent::Upsert(child)) => {
                children.insert(child.id.clone(), child);
            }
            AgentTurnEvent::Subagent(SubagentEvent::Progress { id, entry, .. }) => {
                if entry.item["type"] == "assistant_message" {
                    child_text.push((id, entry.item["text"].clone()));
                }
            }
            AgentTurnEvent::History(entries) => {
                parent_text = entries
                    .into_iter()
                    .filter(|entry| entry.item["type"] == "assistant_message")
                    .collect();
            }
            AgentTurnEvent::Completed(_) => break,
            AgentTurnEvent::Failed | AgentTurnEvent::Cancelled => panic!("parent failed"),
            _ => {}
        }
    }
    assert_eq!(children.len(), 2);
    assert_eq!(children["ses_child"].parent_id, "ses_one");
    assert_eq!(children["ses_nested"].parent_id, "ses_child");
    assert!(
        children
            .values()
            .all(|child| child.descriptor["status"] == "completed")
    );
    assert_eq!(child_text.len(), 2);
    assert!(!format!("{parent_text:?}").contains("CHILD"));
    assert_eq!(session.subagents().len(), 2);
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let discovered = client.subagents(&spec.cwd).await.unwrap();
    assert_eq!(discovered.len(), 2);
    for child in &discovered {
        let history = client
            .inspect_session(child.persistence.as_ref().unwrap(), &spec.cwd)
            .await
            .unwrap();
        assert_eq!(history.parent_id.as_deref(), Some(child.parent_id.as_str()));
        assert!(
            history
                .entries
                .iter()
                .any(|entry| entry.item["text"] == format!("CHILD {}", child.id))
        );
        let mut forged = child.persistence.clone().unwrap();
        forged
            .metadata
            .as_mut()
            .unwrap()
            .get_mut("opencodeChild")
            .unwrap()["parent"] = json!("unrelated");
        assert!(client.inspect_session(&forged, &spec.cwd).await.is_err());
    }
    let mut restored = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert!(restored.subagents().is_empty());
    assert_eq!(client.subagents(&spec.cwd).await.unwrap().len(), 2);
    restored.start_turn("Continue", &spec.config).await.unwrap();
    completed(restored.as_mut()).await;
    restored.close().await.unwrap();
}

#[tokio::test]
async fn an_unloadable_child_session_does_not_prevent_resuming_the_parent() {
    let (root, client, spec) = fixture("subagents");
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("Explore", &spec.config).await.unwrap();
    completed(session.as_mut()).await;
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    std::fs::write(root.path().join("child-load-fails"), "").unwrap();

    let mut restored = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    restored.start_turn("Continue", &spec.config).await.unwrap();
    completed(restored.as_mut()).await;
    restored.close().await.unwrap();
}

#[tokio::test]
async fn settling_a_new_turn_withdraws_an_unanswered_background_permission() {
    let (root, client, spec) = fixture("subagents-idle");
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("Background", &spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    let request = loop {
        if let AgentTurnEvent::PermissionRequested(request) = event(session.as_mut()).await {
            break request;
        }
    };
    session.start_turn("Next", &spec.config).await.unwrap();
    let mut resolved = false;
    loop {
        match event(session.as_mut()).await {
            AgentTurnEvent::PermissionResolved(id) => resolved |= id == request["id"],
            AgentTurnEvent::Completed(_) => break,
            AgentTurnEvent::Failed | AgentTurnEvent::Cancelled => panic!("next turn failed"),
            _ => {}
        }
    }
    session.close().await.unwrap();
    assert!(resolved);
    assert!(
        root.path().join("background-cancelled").exists(),
        "the native callback must receive a cancelled reply instead of waiting forever"
    );
}

#[tokio::test]
async fn settling_a_turn_keeps_receiving_background_children_on_the_live_connection() {
    let (root, client, spec) = fixture("subagents-detach");
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("Background", &spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    assert_eq!(session.subagents()[0].descriptor["status"], "running");
    assert!(
        !root.path().join("background-detached").exists(),
        "settling must not re-attach the root while a background child is running"
    );
    session.start_turn("Next", &spec.config).await.unwrap();
    let mut finished = false;
    loop {
        match event(session.as_mut()).await {
            AgentTurnEvent::Subagent(SubagentEvent::Upsert(child))
                if child.descriptor["status"] == "completed" =>
            {
                finished = true;
            }
            AgentTurnEvent::Completed(_) => break,
            AgentTurnEvent::Failed | AgentTurnEvent::Cancelled => panic!("next turn failed"),
            _ => {}
        }
    }
    session.close().await.unwrap();
    assert!(finished, "the background child never reported completion");
}
