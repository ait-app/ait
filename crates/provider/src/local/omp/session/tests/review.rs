use super::*;

#[tokio::test]
async fn restart_releases_old_action_receipts_before_reusing_native_ids() {
    let (_root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    let receipt = session.actions.insert("ait-omp-1".to_owned(), "get_tree");
    let config = StoredAgentConfig {
        mode_id: Some("ask".to_owned()),
        ..spec.config.clone()
    };
    session.configure(&config).await.unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), receipt)
            .await
            .unwrap(),
        Err(AgentSessionError::Failed)
    );
    assert!(
        !session
            .actions
            .capture(&json!({"type":"response", "id":"ait-omp-1",
        "command":"prompt", "success":true}))
    );
    session.start_turn("hello", &config).await.unwrap();
    assert!(matches!(
        complete(&mut session).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn builtin_waits_for_the_text_query_to_finish_before_writing_a_prompt() {
    let (root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    session
        .native_commands
        .push(json!({"name":"compact", "source":"builtin"}));
    session.command_controls.query(
        "query".into(),
        "message".into(),
        "usage",
        Duration::from_secs(5),
    );
    let calls = root.path().join("agent/calls.jsonl");
    let before = std::fs::read(&calls).unwrap();
    assert_eq!(
        session.start_turn("/compact", &spec.config).await,
        Err(AgentSessionError::Busy)
    );
    assert_eq!(std::fs::read(&calls).unwrap(), before);
    session
        .consume(&json!({"type":"command_output", "text":"query-only"}))
        .unwrap();
    session.consume(&json!({"type":"prompt_result", "id":"query", "agentInvoked":false, "status":"completed"})).unwrap();
    session.start_turn("/compact", &spec.config).await.unwrap();
    let events = complete(&mut session).await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentTurnEvent::Timeline(entry)
        if entry.item["text"] == "query-only"))
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn a_live_btw_rejects_another_question_without_disabling_history_or_cancel() {
    let (root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    session.consume(&json!({"type":"btw_record", "record":{
        "id":"topic", "question":"first", "answer":"", "status":"running", "createdAt":1, "updatedAt":1
    }})).unwrap();
    let calls = root.path().join("agent/calls.jsonl");
    let before = std::fs::read(&calls).unwrap();
    assert_eq!(
        session
            .side_command(&AgentPrompt::text("/btw second"))
            .await,
        Err(AgentSessionError::Busy)
    );
    assert!(matches!(
        session
            .begin_action("btw", &json!({"question":"follow up", "recordId":"topic"}))
            .await,
        Err(AgentSessionError::Busy)
    ));
    assert_eq!(std::fs::read(&calls).unwrap(), before);
    assert_eq!(session.btw.snapshot()["runningId"], "topic");
    let history = session
        .begin_action("get_btw_history", &json!({}))
        .await
        .unwrap();
    let cancel = session
        .begin_action("btw_cancel", &json!({"recordId":"topic"}))
        .await
        .unwrap();
    drop((history, cancel));
    session.close().await.unwrap();
}
