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
