use super::*;

#[tokio::test]
async fn native_settled_denial_does_not_interrupt_and_accepts_next_turn() {
    for (version, aborted_without_idle) in [
        (Version::V1, false),
        (Version::V2, false),
        (Version::V2, true),
    ] {
        let fixture = Fixture::start(version).await;
        let client = OpenCodeClient::new(fixture.binary.clone());
        let spec = spec(&fixture);
        let mut session = client.create_session(&spec).await.unwrap();
        {
            let mut state = fixture.state.lock().unwrap();
            state.reject_interrupt = true;
            state.aborted_completion = aborted_without_idle;
            state.pending_permissions.push(match version {
                Version::V1 => json!({"id":"perm1","sessionID":"ses_one","permission":"bash","patterns":["pwd"]}),
                Version::V2 => json!({"id":"perm1","sessionID":"ses_one","action":"shell","resources":["pwd"]}),
            });
        }
        session.start_turn("hello", &spec.config).await.unwrap();
        deny_permission(session.as_mut()).await;
        let events = drain(session.as_mut()).await;
        assert!(matches!(events.last(), Some(AgentTurnEvent::Cancelled)));
        {
            let mut state = fixture.state.lock().unwrap();
            assert_eq!(state.interrupts, 0);
            assert_eq!(state.submissions, 1);
            assert_eq!(state.replies.len(), 1);
            state.aborted_completion = false;
        }
        session
            .start_turn("after denial", &spec.config)
            .await
            .unwrap();
        assert!(matches!(
            drain(session.as_mut()).await.last(),
            Some(AgentTurnEvent::Completed(_))
        ));
        assert_eq!(fixture.state.lock().unwrap().submissions, 2);
        session.close().await.unwrap();
    }
}

async fn deny_permission(session: &mut dyn AgentSession) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Some(AgentTurnEvent::PermissionRequested(request)) = session.poll_turn().unwrap()
            {
                session
                    .respond_permission(
                        request["id"].as_str().unwrap(),
                        &json!({"behavior":"deny","selectedActionId":"deny"}),
                    )
                    .await
                    .unwrap();
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn idle_denial_without_current_input_evidence_closes_writer_without_interrupt_or_replay() {
    for version in [Version::V1, Version::V2] {
        let fixture = Fixture::start(version).await;
        let client = OpenCodeClient::new(fixture.binary.clone());
        let spec = spec(&fixture);
        let mut session = client.create_session(&spec).await.unwrap();
        {
            let mut state = fixture.state.lock().unwrap();
            state.omit_input_history = true;
            state.pending_permissions.push(match version {
                Version::V1 => json!({"id":"perm1","sessionID":"ses_one","permission":"bash","patterns":["pwd"]}),
                Version::V2 => json!({"id":"perm1","sessionID":"ses_one","action":"shell","resources":["pwd"]}),
            });
        }
        session.start_turn("hello", &spec.config).await.unwrap();
        deny_permission(session.as_mut()).await;
        assert!(matches!(
            drain(session.as_mut()).await.last(),
            Some(AgentTurnEvent::Failed)
        ));
        assert!(
            session
                .start_turn("must not replay", &spec.config)
                .await
                .is_err()
        );
        {
            let state = fixture.state.lock().unwrap();
            assert_eq!(state.interrupts, 0);
            assert_eq!(state.submissions, 1);
        }
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn declined_permission_requires_native_interrupt_acknowledgement_in_both_protocols() {
    for version in [Version::V1, Version::V2] {
        for reject_interrupt in [false, true] {
            let fixture = Fixture::start(version).await;
            let client = OpenCodeClient::new(fixture.binary.clone());
            let spec = spec(&fixture);
            let mut session = client.create_session(&spec).await.unwrap();
            {
                let mut state = fixture.state.lock().unwrap();
                state.reject_interrupt = reject_interrupt;
                state.stream_after_permission = true;
                state.pending_permissions.push(match version {
                    Version::V1 => json!({"id":"perm1","sessionID":"ses_one","permission":"bash","patterns":["pwd"]}),
                    Version::V2 => json!({"id":"perm1","sessionID":"ses_one","action":"shell","resources":["pwd"]}),
                });
            }
            session.start_turn("hello", &spec.config).await.unwrap();
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    if matches!(
                        session.poll_turn().unwrap(),
                        Some(AgentTurnEvent::PermissionRequested(_))
                    ) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
            session
                .respond_permission(
                    "perm1",
                    &json!({"behavior":"deny","selectedActionId":"deny"}),
                )
                .await
                .unwrap();

            let events = drain(session.as_mut()).await;
            assert_eq!(fixture.state.lock().unwrap().interrupts, 1);
            if reject_interrupt {
                assert!(matches!(events.last(), Some(AgentTurnEvent::Failed)));
                assert!(
                    session
                        .start_turn("must not replay", &spec.config)
                        .await
                        .is_err()
                );
                assert_eq!(fixture.state.lock().unwrap().submissions, 1);
            } else {
                assert!(matches!(events.last(), Some(AgentTurnEvent::Cancelled)));
                session
                    .start_turn("continue after denial", &spec.config)
                    .await
                    .unwrap();
                assert!(matches!(
                    drain(session.as_mut()).await.last(),
                    Some(AgentTurnEvent::Completed(_))
                ));
                assert_eq!(fixture.state.lock().unwrap().submissions, 2);
            }
            session.close().await.unwrap();
        }
    }
}
