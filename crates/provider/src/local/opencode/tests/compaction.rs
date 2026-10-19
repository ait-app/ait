//! Native compaction continuations share the accepted input's stable turn identity.
use super::*;

#[tokio::test]
async fn automatic_compaction_continuations_settle_and_survive_replay_and_resume() {
    let (root, client, spec) = fixture("auto-compaction");
    let mut session = client.create_session(&spec).await.unwrap();
    let timeline = crate::storage::timeline::Timeline::memory().unwrap();
    let turn = session
        .start_turn("initial input", &spec.config)
        .await
        .unwrap();
    let history = recorded_turn(session.as_mut(), &timeline, &turn).await;
    assert_eq!(history.len(), 6);
    assert!(
        history
            .iter()
            .all(|entry| entry.turn_id.as_deref() == Some(&turn))
    );
    let handle = session.persistence().unwrap();
    for user in ["user1", "continue0", "continue1"] {
        assert_eq!(
            handle.metadata.as_ref().unwrap()["opencode"]["clients"][user],
            turn
        );
    }
    session.close().await.unwrap();
    assert_replay(&history, &client.history(&handle, &spec.cwd).await.unwrap());

    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    let next = resumed.start_turn("follow up", &spec.config).await.unwrap();
    let history = recorded_turn(resumed.as_mut(), &timeline, &next).await;
    assert_eq!(history.len(), 8);
    assert!(
        history[..6]
            .iter()
            .all(|entry| entry.turn_id.as_deref() == Some(&turn))
    );
    assert!(
        history[6..]
            .iter()
            .all(|entry| entry.turn_id.as_deref() == Some(&next))
    );
    let resumed_handle = resumed.persistence().unwrap();
    resumed.close().await.unwrap();
    assert_replay(
        &history,
        &client.history(&resumed_handle, &spec.cwd).await.unwrap(),
    );
    assert_eq!(
        requests(&root)
            .iter()
            .filter(|message| message["method"] == "session/prompt")
            .count(),
        2
    );
}

#[tokio::test]
async fn automatic_compaction_continuations_respect_the_persisted_identity_budget() {
    for existing in [4093, 4094] {
        let (_root, client, spec) = fixture("auto-compaction");
        let mut initial = client.create_session(&spec).await.unwrap();
        let mut handle = initial.persistence().unwrap();
        initial.close().await.unwrap();
        let clients: BTreeMap<_, _> = (0..existing)
            .map(|index| (format!("stored-{index}"), "previous-turn".to_owned()))
            .collect();
        handle
            .metadata
            .as_mut()
            .unwrap()
            .get_mut("opencode")
            .unwrap()["clients"] = json!(clients);
        let mut session = client
            .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
            .await
            .unwrap();
        session.start_turn("compact", &spec.config).await.unwrap();
        if existing == 4093 {
            completed(session.as_mut()).await;
            let handle = session.persistence().unwrap();
            assert_eq!(session::clients(&handle).unwrap().len(), 4096);
            assert_eq!(
                session
                    .start_turn("beyond budget", &spec.config)
                    .await
                    .unwrap_err(),
                AgentSessionError::Rejected,
            );
            session.close().await.unwrap();
        } else {
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    match session.poll_turn() {
                        Err(error) => {
                            assert_eq!(error, AgentSessionError::Failed);
                            break;
                        }
                        Ok(Some(AgentTurnEvent::Completed(_))) => {
                            panic!("identity budget exceeded")
                        }
                        Ok(Some(_)) => {}
                        Ok(None) => tokio::time::sleep(Duration::from_millis(5)).await,
                    }
                }
            })
            .await
            .unwrap();
        }
    }
}

#[tokio::test]
async fn standalone_acp_compactions_keep_distinct_turns_through_replay_and_resume() {
    let (_root, client, spec) = fixture("normal");
    let mut session = client.create_session(&spec).await.unwrap();
    let mut compactions = BTreeMap::new();
    let mut history = Vec::new();
    for prompt in ["seed", "/compact", "next input", "/compact"] {
        let turn = session.start_turn(prompt, &spec.config).await.unwrap();
        let answer = loop {
            match event(session.as_mut()).await {
                AgentTurnEvent::History(entries) => history = entries,
                AgentTurnEvent::Completed(text) => break text,
                AgentTurnEvent::Failed | AgentTurnEvent::Cancelled => panic!("fixture failed"),
                _ => {}
            }
        };
        if prompt == "/compact" {
            assert_eq!(answer, None);
            let marker = history
                .iter()
                .rev()
                .find(|entry| entry.item["type"] == "compaction")
                .unwrap();
            assert_eq!(marker.turn_id.as_deref(), Some(turn.as_str()));
            compactions.insert(
                marker.item["compactionId"].as_str().unwrap().to_owned(),
                turn,
            );
        }
    }
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    assert_eq!(compactions.len(), 2);
    assert_replay(&history, &client.history(&handle, &spec.cwd).await.unwrap());
    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    resumed
        .start_turn("after resume", &spec.config)
        .await
        .unwrap();
    let history = completed(resumed.as_mut()).await;
    for marker in history
        .iter()
        .filter(|entry| entry.item["type"] == "compaction")
    {
        assert_eq!(
            marker.turn_id.as_ref(),
            compactions.get(marker.item["compactionId"].as_str().unwrap())
        );
    }
    resumed.close().await.unwrap();
}
