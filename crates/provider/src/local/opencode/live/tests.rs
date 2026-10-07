use super::super::{OpenCodeExecutionLimits, client::OpenCodeClient, tests::fixture::Fixture};
use super::*;
use crate::ports::agent_session::{AgentClient, AgentResumePurpose, AgentSessionSpec};

mod ordering;
mod streaming;
mod tool_ordering;

fn spec(fixture: &Fixture) -> AgentSessionSpec {
    AgentSessionSpec {
        provider: "opencode".into(),
        cwd: fixture.cwd.to_string_lossy().into_owned(),
        config: StoredAgentConfig {
            model: Some("local/test-model".into()),
            ..Default::default()
        },
    }
}

async fn drain(session: &mut dyn AgentSession) -> Vec<AgentTurnEvent> {
    tokio::time::timeout(Duration::from_secs(8), async {
        let mut events = Vec::new();
        loop {
            if let Some(event) = session.poll_turn().unwrap() {
                let terminal = matches!(
                    event,
                    AgentTurnEvent::Completed(_)
                        | AgentTurnEvent::Cancelled
                        | AgentTurnEvent::Failed
                );
                events.push(event);
                if terminal {
                    return events;
                }
            } else {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn server_ports_discover_run_multiple_turns_restore_and_read_without_submission() {
    for version in [Version::V1, Version::V2] {
        let fixture = Fixture::start(version).await;
        let client = OpenCodeClient::new(fixture.binary.clone());
        let spec = spec(&fixture);
        assert!(client.is_available().await.unwrap());
        let details = client.discover(&spec.cwd).await.unwrap();
        assert_eq!(details.models[0]["id"], "local/test-model");
        assert_eq!(details.models[0]["provider"], "opencode");
        assert_eq!(details.modes[0]["id"], "build");
        let mut session = client.create_session(&spec).await.unwrap();
        assert_eq!(fixture.state.lock().unwrap().submissions, 0);
        for index in 0..2 {
            let prompt = AgentPrompt {
                text: format!("hello {index}"),
                client_message_id: Some(format!("client-{index}")),
                ..Default::default()
            };
            let turn = session.start_input(&prompt, &spec.config).await.unwrap();
            assert!(session.start_turn("busy", &spec.config).await.is_err());
            assert!(session.cancel_turn("wrong-turn").await.is_err());
            let events = drain(session.as_mut()).await;
            assert!(
                matches!(events.last(),Some(AgentTurnEvent::Completed(Some(text))) if text=="answer")
            );
            let items = events
                .iter()
                .filter_map(|event| {
                    if let AgentTurnEvent::Timeline(item) = event {
                        Some(item)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            assert_eq!(items.len(), 2);
            assert_eq!(items[0].item["clientMessageId"], format!("client-{index}"));
            assert_eq!(items[0].turn_id.as_deref(), Some(turn.as_str()));
        }
        let handle = session.persistence().unwrap();
        assert!(
            handle.native_handle.as_ref().unwrap().is_string(),
            "nativeHandle must satisfy the frontend schema"
        );
        session.close().await.unwrap();
        let history = client.history(&handle, &spec.cwd).await.unwrap();
        assert_eq!(history.len(), 4);
        assert_eq!(history[0].item["clientMessageId"], "client-0");
        let inspection = client.inspect_session(&handle, &spec.cwd).await.unwrap();
        assert!(!inspection.active);
        assert_eq!(inspection.entries, history);
        let mut reader = client
            .resume_session(&handle, &spec, AgentResumePurpose::History)
            .await
            .unwrap();
        assert!(reader.start_turn("forbidden", &spec.config).await.is_err());
        reader.close().await.unwrap();
        assert_eq!(fixture.state.lock().unwrap().submissions, 2);
        let mut resumed = client
            .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
            .await
            .unwrap();
        resumed
            .start_turn("after restart", &spec.config)
            .await
            .unwrap();
        assert!(matches!(
            drain(resumed.as_mut()).await.last(),
            Some(AgentTurnEvent::Completed(_))
        ));
        resumed.close().await.unwrap();
        assert_eq!(fixture.state.lock().unwrap().submissions, 3);
    }
}

#[tokio::test]
async fn ambiguous_admission_reconciles_without_replay_and_unsupported_inputs_fail_before_submission()
 {
    let fixture = Fixture::start(Version::V2).await;
    let client = OpenCodeClient::new(fixture.binary.clone());
    let spec = spec(&fixture);
    let mut session = client.create_session(&spec).await.unwrap();
    let prompt = AgentPrompt {
        text: "hello".into(),
        output_schema: Some(json!({"type":"object"})),
        ..Default::default()
    };
    assert_eq!(
        session
            .start_input(&prompt, &spec.config)
            .await
            .unwrap_err(),
        AgentSessionError::Rejected
    );
    let mut config = spec.config.clone();
    config.mode_id = Some("unsupported-agent".into());
    assert!(client.validate_config(&config).is_err());
    assert!(session.start_turn("hello", &config).await.is_err());
    assert_eq!(fixture.state.lock().unwrap().submissions, 0);
    fixture.state.lock().unwrap().reject_ack = true;
    session.start_turn("hello", &spec.config).await.unwrap();
    assert!(matches!(
        drain(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    assert_eq!(fixture.state.lock().unwrap().submissions, 1);
    session.close().await.unwrap();
    assert!(session.start_turn("closed", &spec.config).await.is_err());
    assert!(session.runtime_info().await.is_err());
    assert!(
        client
            .with_execution_limits(OpenCodeExecutionLimits {
                max_steps: 0,
                ..Default::default()
            })
            .is_err()
    );
}

#[tokio::test]
async fn server_approvals_reject_wider_authority_resolve_once_and_cancel_with_native_acknowledgement()
 {
    for behavior in ["allow", "always", "deny", "cancel"] {
        let fixture = Fixture::start(Version::V2).await;
        let client = OpenCodeClient::new(fixture.binary.clone());
        let spec = spec(&fixture);
        let mut session = client.create_session(&spec).await.unwrap();
        fixture
            .state
            .lock()
            .unwrap()
            .pending_permissions
            .push(json!({"id":"perm1","sessionID":"ses_one","action":"shell","resources":["pwd"],"save":if behavior == "always" {json!(["pwd"])} else {json!([])}}));
        let turn = session.start_turn("hello", &spec.config).await.unwrap();
        let request = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Some(AgentTurnEvent::PermissionRequested(request)) =
                    session.poll_turn().unwrap()
                {
                    return request;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            request["actions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|action| action["id"] == "always"),
            behavior == "always"
        );
        if behavior != "always" {
            assert!(
                session
                    .respond_permission(
                        "perm1",
                        &json!({"behavior":"allow","selectedActionId":"always"})
                    )
                    .await
                    .is_err()
            );
        }
        assert_eq!(session.pending_permissions(), vec![request]);
        assert_invalid_approval_responses(session.as_mut()).await;
        if behavior == "cancel" {
            session.cancel_turn(&turn).await.unwrap();
            assert!(matches!(
                drain(session.as_mut()).await.last(),
                Some(AgentTurnEvent::Cancelled)
            ));
            session
                .start_turn("continue after cancel", &spec.config)
                .await
                .unwrap();
            assert!(matches!(
                drain(session.as_mut()).await.last(),
                Some(AgentTurnEvent::Completed(_))
            ));
        } else {
            let response = if behavior == "deny" {
                json!({"behavior":"deny","selectedActionId":"deny","message":"Denied by user"})
            } else {
                json!({"behavior":"allow","selectedActionId":if behavior == "always" {"always"} else {"allow"}})
            };
            session
                .respond_permission("perm1", &response)
                .await
                .unwrap();
            assert!(
                session
                    .respond_permission("perm1", &json!({"behavior":"allow"}))
                    .await
                    .is_err()
            );
            let events = drain(session.as_mut()).await;
            assert!(events.iter().any(
                |event| matches!(event,AgentTurnEvent::PermissionResolved(id) if id=="perm1")
            ));
            assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
            assert_eq!(
                fixture.state.lock().unwrap().replies,
                vec![
                    json!({"decision":if behavior=="allow" {"once"} else if behavior=="always" {"always"} else {"reject"}})
                ]
            );
        }
        assert!(session.pending_permissions().is_empty());
        assert_eq!(
            fixture.state.lock().unwrap().submissions,
            if behavior == "cancel" { 2 } else { 1 }
        );
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn streamed_text_uses_the_final_native_item_key_in_both_protocols() {
    for version in [Version::V1, Version::V2] {
        let fixture = Fixture::start(version).await;
        let client = OpenCodeClient::new(fixture.binary.clone());
        let spec = spec(&fixture);
        let mut session = client.create_session(&spec).await.unwrap();
        {
            let mut state = fixture.state.lock().unwrap();
            state.busy = true;
            state.stream_text = true;
        }
        let turn = session.start_turn("hello", &spec.config).await.unwrap();
        let (observation, progress) = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Some(AgentTurnEvent::Progress { observation, entry }) =
                    session.poll_turn().unwrap()
                {
                    return (observation, entry);
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(progress.item["text"], "ans");
        assert_eq!(progress.turn_id.as_deref(), Some(turn.as_str()));
        let timeline = crate::storage::timeline::Timeline::memory().unwrap();
        timeline
            .progress("agent", "opencode", &observation, &progress)
            .unwrap();
        fixture.state.lock().unwrap().busy = false;
        let events = drain(session.as_mut()).await;
        assert!(events.iter().any(|event|matches!(event,AgentTurnEvent::Timeline(item) if item.key==progress.key && item.item["text"]=="answer")));
        for event in events {
            if let AgentTurnEvent::Timeline(item) = event {
                timeline.append("agent", "opencode", &[item]).unwrap();
            }
        }
        let rows = timeline.read("agent").unwrap().1;
        let text = rows
            .iter()
            .filter(|row| row.entry.item["type"] == "assistant_message")
            .map(|row| row.entry.item["text"].as_str().unwrap())
            .collect::<String>();
        assert_eq!(text, "answer");
        session.close().await.unwrap();
    }
}

async fn assert_invalid_approval_responses(session: &mut dyn AgentSession) {
    assert!(
        session
            .respond_permission("stale", &json!({"behavior":"allow"}))
            .await
            .is_err()
    );
    assert!(
        session
            .respond_permission(
                "perm1",
                &json!({"behavior":"allow","updatedPermissions":[{}]})
            )
            .await
            .is_err()
    );
    assert!(
        session
            .respond_permission(
                "perm1",
                &json!({"behavior":"allow","selectedActionId":"deny"})
            )
            .await
            .is_err()
    );
}
