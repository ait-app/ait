use super::*;
use crate::session;
use crate::tests::fixture;
use provider_core::ports::agent_session::AgentSession;

#[test]
fn membership_and_native_acknowledgements_are_classified_conservatively() {
    let running = json!({"success":true,"data":{"subagents":[{"id":"child","status":"running","sessionFile":"/tmp/file"}]}});
    assert_eq!(membership(&running, "child"), Ok(true));
    assert_eq!(membership(&running, "other"), Ok(false));
    assert_eq!(
        membership(&json!({"success":true,"data":{}}), "child"),
        Err(ChildControlFailure::Uncertain)
    );
    assert_eq!(
        membership(&json!({"success":false}), "child"),
        Err(ChildControlFailure::Rejected)
    );
    assert_eq!(
        disposition(
            Kind::Guide,
            &json!({"success":false,"error":"Subagent not running: child"})
        ),
        Disposition::NotRunning
    );
    assert_eq!(
        disposition(
            Kind::Stop,
            &json!({"success":true,"data":{"cancelled":false}})
        ),
        Disposition::NotRunning
    );
    assert_eq!(
        disposition(Kind::Stop, &json!({"success":true,"data":{}})),
        Disposition::Failed(ChildControlFailure::Uncertain)
    );
}

#[test]
fn incomplete_membership_cannot_claim_the_child_is_not_running() {
    for child in [
        json!({"id":"child","status":"running"}),
        json!({"id":"child","status":"pending","sessionFile":""}),
        json!({"id":"child","status":"running","sessionFile":"   "}),
        json!({"id":"child","sessionFile":"/tmp/file"}),
        json!({"id":"child","status":"unknown","sessionFile":"/tmp/file"}),
        json!({"status":"completed"}),
        json!({"id":"","status":"completed"}),
        json!({"id":"../bad","status":"completed"}),
    ] {
        let snapshot = json!({"success":true,"data":{"subagents":[child]}});
        assert_eq!(
            membership(&snapshot, "child"),
            Err(ChildControlFailure::Uncertain),
            "{snapshot}"
        );
    }
    let duplicate = json!({"success":true,"data":{"subagents":[
        {"id":"child","status":"completed"},
        {"id":"child","status":"running","sessionFile":"/tmp/file"}
    ]}});
    assert_eq!(
        membership(&duplicate, "child"),
        Err(ChildControlFailure::Uncertain)
    );
    for status in ["completed", "failed", "aborted", "cancelled", "canceled"] {
        let snapshot =
            json!({"success":true,"data":{"subagents":[{"id":"child","status":status}]}});
        assert_eq!(membership(&snapshot, "child"), Ok(false));
    }
    assert_eq!(
        membership(&json!({"success":true,"data":{"subagents":[]}}), "child"),
        Ok(false)
    );
}

#[tokio::test]
async fn parent_owned_control_checks_membership_and_preserves_a_running_turn() {
    let (_root, client, spec) = fixture();
    let mut session = session::open(&client, &spec, None).await.unwrap();
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    let parent = session.persistence().unwrap().session_id;
    assert!(matches!(
        session.begin_child_stop("other/child"),
        Err(AgentSessionError::Rejected)
    ));
    assert!(matches!(
        session.begin_child_guide(&format!("{parent}/child"), "  \n"),
        Err(AgentSessionError::Rejected)
    ));
    let receipt = session
        .begin_child_guide(&format!("{parent}/child"), "hello")
        .unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(AgentTurnEvent::ChildControl { token, outcome }) =
                session.poll_turn().unwrap()
                && token == receipt.token
            {
                break outcome;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        result,
        ChildControlOutcome::Guide(Ok(ChildGuideOutcome::NotRunning))
    );
    assert_eq!(session.active.as_deref(), Some(turn.as_str()));
    session.close().await.unwrap();
}

#[tokio::test]
async fn an_expired_membership_reply_never_schedules_a_late_control() {
    let (_root, client, spec) = fixture();
    let mut session = session::open(&client, &spec, None).await.unwrap();
    let parent = session.persistence().unwrap().session_id;
    let receipt = session
        .begin_child_stop(&format!("{parent}/child"))
        .unwrap();
    let (id, pending) = session.child_controls.pending.iter_mut().next().unwrap();
    let id = id.clone();
    pending.deadline = Instant::now() - std::time::Duration::from_millis(1);
    assert!(session.child_controls.response(
        &mut session.transport,
        &json!({
            "id":id,"command":"get_subagents","success":true,
            "data":{"subagents":[{"id":"child","status":"running","sessionFile":"/tmp/file"}]}
        }),
        &mut session.stream.events
    ));
    assert!(session.child_controls.is_empty());
    assert!(session.stream.events.iter().any(|event| matches!(event,
        AgentTurnEvent::ChildControl { token, outcome:ChildControlOutcome::Stop(Err(ChildControlFailure::Uncertain)) } if token == &receipt.token)));
    session.close().await.unwrap();
}
