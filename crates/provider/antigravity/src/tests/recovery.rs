use super::*;

#[tokio::test]
async fn failed_drivers_emit_one_terminal_event_and_resume_without_replaying_the_prompt() {
    for text in ["fail-once", "malformed-once", "crash-once"] {
        let (directory, client, spec) = fixture();
        let mut session = client.create_session(&spec).await.unwrap();
        let handle = session.persistence().unwrap();
        session.start_turn(text, &spec.config).await.unwrap();
        while event(session.as_mut()).await.unwrap() != AgentTurnEvent::Failed {}
        for _ in 0..5 {
            assert_eq!(session.poll_turn().unwrap(), None, "{text}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        session.start_turn("recovered", &spec.config).await.unwrap();
        completed(session.as_mut()).await;
        assert_eq!(session.persistence().unwrap(), handle);
        session.close().await.unwrap();
        let calls = logs(&directory);
        assert_eq!(
            calls
                .iter()
                .filter(|call| call.get("input").is_some())
                .count(),
            2
        );
        let launches: Vec<_> = calls
            .iter()
            .filter_map(|call| call["args"].as_array())
            .filter(|args| args.iter().any(|arg| arg == "--input-format"))
            .collect();
        assert_eq!(launches.len(), 2, "{text}");
        assert!(launches[1].iter().any(|arg| arg == &handle.session_id));
    }
}

#[tokio::test]
async fn cancellation_without_native_acknowledgement_fails_once_and_unblocks_the_session() {
    assert_failed_cancellation("cancel-exit").await;
}

#[tokio::test]
async fn cancellation_timeout_fails_once_and_unblocks_the_session() {
    assert_failed_cancellation("cancel-timeout").await;
}

async fn assert_failed_cancellation(scenario: &str) {
    let (_directory, mut client, spec) = fixture();
    client.environment = AgentEnvironment::try_from(BTreeMap::from([(
        "AGY_FIXTURE_SCENARIO".to_owned(),
        scenario.to_owned(),
    )]))
    .unwrap();
    let mut session = client.create_session(&spec).await.unwrap();
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    assert!(session.cancel_turn(&turn).await.is_err());
    assert!(matches!(
        event(session.as_mut()).await.unwrap(),
        AgentTurnEvent::RuntimeInfo(_)
    ));
    assert!(
        matches!(event(session.as_mut()).await.unwrap(), AgentTurnEvent::Timeline(entry)
        if entry.item["type"] == "user_message" && entry.item["text"] == "wait")
    );
    assert_eq!(
        event(session.as_mut()).await.unwrap(),
        AgentTurnEvent::Failed
    );
    assert_eq!(session.poll_turn().unwrap(), None);
    session.start_turn("recovered", &spec.config).await.unwrap();
    completed(session.as_mut()).await;
    session.close().await.unwrap();
}

#[tokio::test]
async fn configuration_reconnect_resets_process_denials() {
    let (_directory, mut client, mut spec) = fixture();
    client.environment = AgentEnvironment::try_from(BTreeMap::from([(
        "AGY_FIXTURE_SCENARIO".to_owned(),
        "denied".to_owned(),
    )]))
    .unwrap();
    let mut session = client.create_session(&spec).await.unwrap();
    for mode in [None, Some("full-access".to_owned())] {
        spec.config.mode_id = mode;
        session.start_turn("denied", &spec.config).await.unwrap();
        loop {
            match event(session.as_mut()).await.unwrap() {
                AgentTurnEvent::Failed => break,
                AgentTurnEvent::Completed(_) => panic!("new process denial was ignored"),
                _ => {}
            }
        }
    }
    session.close().await.unwrap();
}
