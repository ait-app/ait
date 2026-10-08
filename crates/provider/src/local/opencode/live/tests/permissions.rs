use super::*;

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
