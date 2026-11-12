//! Native history, listing and subagent transcripts (B7) against the offline peer.
use super::*;
use provider_core::ports::controls::SubagentEvent;

#[tokio::test]
async fn live_children_match_the_transcripts_history_discovers() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("subagents", &spec.config).await.unwrap();
    let events = complete(session.as_mut()).await;
    let parent = session.persistence().unwrap();
    let live = session.subagents();
    session.close().await.unwrap();

    assert_eq!(live.len(), 1);
    let child = &live[0];
    let id = format!("{}/worker", parent.session_id);
    assert_eq!(child.id, id);
    assert_eq!(child.descriptor["title"], "Task · model (test)");
    assert_eq!(child.descriptor["description"], "Review the change");
    // While the task runs, its card names the child its events reported.
    assert!(events.iter().any(|event| matches!(event,
        AgentTurnEvent::Progress { entry, .. } if entry.item["status"] == "running"
            && entry.item["detail"]["childSessionId"] == id.as_str())));
    let card = events
        .iter()
        .find_map(|event| match event {
            AgentTurnEvent::Timeline(entry) if entry.item["name"] == "task" => Some(&entry.item),
            _ => None,
        })
        .unwrap();
    assert_eq!(card["detail"]["childSessionId"], id.as_str());
    let live_child: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Subagent(SubagentEvent::Timeline { entry, .. }) => Some(entry.clone()),
            _ => None,
        })
        .collect();

    let discovered = client.subagents(&spec.cwd).await.unwrap();
    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0].id, id);
    assert_eq!(discovered[0].parent_id, parent.session_id);
    assert_eq!(discovered[0].persistence, child.persistence);
    assert_eq!(discovered[0].descriptor["title"], child.descriptor["title"]);
    assert_eq!(discovered[0].descriptor["status"], "completed");
    assert_eq!(discovered[0].descriptor["toolCallId"], "task-1");
    let history = client
        .inspect_session(child.persistence.as_ref().unwrap(), &spec.cwd)
        .await
        .unwrap();
    assert_eq!(
        history.parent_id.as_deref(),
        Some(parent.session_id.as_str())
    );
    assert_eq!(history.descriptor.provider_handle_id, id);
    assert_eq!(history.entries.last(), live_child.last());
    // The parent's history shows the same task card.
    let parent_history = client.history(&parent, &spec.cwd).await.unwrap();
    assert!(parent_history.iter().any(|entry| entry.item == *card));
}

#[tokio::test]
async fn a_refused_event_subscription_falls_back_to_progress() {
    for refused in ["events", "events,progress"] {
        let (root, client, spec) = fixture_with(&[("OMP_FIXTURE_REJECT_SUBSCRIPTION", refused)]);

        let mut session = client.create_session(&spec).await.unwrap();
        session.close().await.unwrap();

        let levels: Vec<_> = calls_after(root.path(), 0)
            .into_iter()
            .filter(|call| call["type"] == "set_subagent_subscription")
            .map(|call| call["level"].clone())
            .collect();
        assert_eq!(levels, [json!("events"), json!("progress")]);
    }
}

#[tokio::test]
async fn omp_todo_state_is_shown_at_attach_and_after_turns() {
    let phases = r#"[{"name":"Plan","tasks":[{"content":"Resume","status":"in_progress"}]}]"#;
    let (_root, client, spec) = fixture_with(&[("OMP_FIXTURE_TODO_PHASES", phases)]);
    let mut session = client.create_session(&spec).await.unwrap();
    let AgentTurnEvent::Timeline(attached) = next(session.as_mut()).await.unwrap() else {
        panic!("expected the attached todo list");
    };
    assert_eq!(attached.item["items"][0]["text"], "Resume");

    session
        .start_turn("todo-state", &spec.config)
        .await
        .unwrap();
    complete(session.as_mut()).await;
    let AgentTurnEvent::Timeline(after) = next(session.as_mut()).await.unwrap() else {
        panic!("expected the todo list read after the turn");
    };
    assert_eq!(after.item["type"], "todo");
    assert_eq!(after.item["items"][0]["text"], "Check state");
    // An unchanged state shows nothing after the next turn.
    session.start_turn("again", &spec.config).await.unwrap();
    complete(session.as_mut()).await;
    for _ in 0..20 {
        assert!(session.poll_turn().unwrap().is_none());
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    session.close().await.unwrap();
}

#[tokio::test]
async fn listing_reads_omps_title_slot_and_skips_child_transcripts() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("subagents", &spec.config).await.unwrap();
    complete(session.as_mut()).await;
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let file = handle.metadata.as_ref().unwrap()["sessionFile"]
        .as_str()
        .unwrap()
        .to_owned();
    // OMP 18.18.8 rewrites the padded title slot on its first line in place.
    let contents = std::fs::read_to_string(&file).unwrap();
    std::fs::write(
        &file,
        format!(
            "{}\n{contents}",
            json!({"type":"title","v":1,"title":"Named session"})
        ),
    )
    .unwrap();

    let sessions = client
        .list_sessions(&ListOptions {
            cwd: Some(spec.cwd.clone()),
            scan_limit: 10,
        })
        .await
        .unwrap();

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].provider_handle_id, handle.session_id);
    assert_eq!(sessions[0].title.as_deref(), Some("Named session"));
    assert_eq!(
        sessions[0].first_prompt_preview.as_deref(),
        Some("subagents")
    );
    let history = client.inspect_session(&handle, &spec.cwd).await.unwrap();
    assert_eq!(history.descriptor.title.as_deref(), Some("Named session"));
}
