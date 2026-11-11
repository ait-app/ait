//! Parent-owned child guide/stop against the offline native peer.
use super::*;
use crate::ports::controls::{
    ChildControlFailure, ChildControlOutcome, ChildGuideOutcome, ChildStopOutcome,
};

fn calls(root: &tempfile::TempDir, kind: &str) -> Vec<Value> {
    std::fs::read_to_string(root.path().join("agent/calls.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|call| call["type"] == kind)
        .collect()
}

/// Open a session whose native peer reports one running and one completed child.
async fn subagents() -> (tempfile::TempDir, Box<dyn AgentSession>, AgentSessionSpec) {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("subagent-controls", &spec.config)
        .await
        .unwrap();
    complete(session.as_mut()).await;
    assert_eq!(
        session.subagents().len(),
        2,
        "the native child lifecycle was not observed"
    );
    (root, session, spec)
}

/// Service the control hook and drain events until the answer for `token` arrives.
async fn control(session: &mut dyn AgentSession, token: &str) -> ChildControlOutcome {
    for _ in 0..500 {
        session.service_controls().await.unwrap();
        while let Some(event) = session.poll_turn().unwrap() {
            if let AgentTurnEvent::ChildControl {
                token: event_token,
                outcome,
            } = event
                && event_token == token
            {
                return outcome;
            }
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    panic!("the native child control never answered");
}

#[tokio::test]
async fn guidance_is_delivered_once_to_the_running_native_child_only() {
    let (root, mut session, _spec) = subagents().await;
    let receipt = session
        .guide_child("worker", "focus on the failing test")
        .await
        .unwrap();
    assert_eq!(
        control(session.as_mut(), &receipt.token).await,
        ChildControlOutcome::Guide(Ok(ChildGuideOutcome::Accepted))
    );
    let steers = calls(&root, "steer_subagent");
    assert_eq!(steers.len(), 1, "guidance must be written exactly once");
    assert_eq!(steers[0]["subagentId"], "worker");
    assert_eq!(steers[0]["message"], "focus on the failing test");

    // A completed child is a not-running fact, and no native write may target it.
    let receipt = session.guide_child("done", "again").await.unwrap();
    assert_eq!(
        control(session.as_mut(), &receipt.token).await,
        ChildControlOutcome::Guide(Ok(ChildGuideOutcome::NotRunning))
    );
    // A child this process never reported is the same typed fact.
    let receipt = session.guide_child("elsewhere", "hijack").await.unwrap();
    assert_eq!(
        control(session.as_mut(), &receipt.token).await,
        ChildControlOutcome::Guide(Ok(ChildGuideOutcome::NotRunning))
    );
    assert_eq!(calls(&root, "steer_subagent").len(), 1);
    assert!(
        session
            .guide_child("worker", "   ")
            .await
            .is_err_and(|error| error == AgentSessionError::Rejected)
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn stop_cancels_one_running_child_and_reports_terminal_children_as_facts() {
    let (root, mut session, _spec) = subagents().await;
    let receipt = session.stop_child("worker").await.unwrap();
    assert_eq!(
        control(session.as_mut(), &receipt.token).await,
        ChildControlOutcome::Stop(Ok(ChildStopOutcome::Cancelled))
    );
    assert_eq!(calls(&root, "cancel_subagent").len(), 1);
    // The native lifecycle event is what proves termination; the receipt only accepted it.
    assert!(
        session
            .subagents()
            .iter()
            .any(|child| child.id == "worker" && child.descriptor["status"] == "canceled")
    );

    // A terminal child answers `not_running` without another native write.
    let receipt = session.stop_child("worker").await.unwrap();
    assert_eq!(
        control(session.as_mut(), &receipt.token).await,
        ChildControlOutcome::Stop(Ok(ChildStopOutcome::NotRunning))
    );
    let receipt = session.stop_child("elsewhere").await.unwrap();
    assert_eq!(
        control(session.as_mut(), &receipt.token).await,
        ChildControlOutcome::Stop(Ok(ChildStopOutcome::NotRunning))
    );
    assert_eq!(calls(&root, "cancel_subagent").len(), 1);
    session.close().await.unwrap();
}

#[tokio::test]
async fn a_refused_guidance_rejects_without_a_second_native_write() {
    let (root, mut session, _spec) = subagents().await;
    let receipt = session.guide_child("worker", "refuse").await.unwrap();
    assert_eq!(
        control(session.as_mut(), &receipt.token).await,
        ChildControlOutcome::Guide(Err(ChildControlFailure::Rejected))
    );
    assert_eq!(calls(&root, "steer_subagent").len(), 1);
    session.close().await.unwrap();
}

#[tokio::test]
async fn a_permission_question_is_answerable_while_a_child_control_receipt_is_pending() {
    let (root, mut session, spec) = subagents().await;
    // The fixture withholds the cancel answer until the host answers the question it raises.
    std::fs::write(root.path().join("agent/hold-control-answer"), "").unwrap();
    let receipt = session.stop_child("worker").await.unwrap();

    // The control is written but unanswered: no typed outcome may exist yet.
    let mut answered = false;
    for _ in 0..200 {
        session.service_controls().await.unwrap();
        while let Some(event) = session.poll_turn().unwrap() {
            assert!(
                !matches!(event, AgentTurnEvent::ChildControl { .. }),
                "the control answered before its native answer arrived"
            );
        }
        let pending = session.pending_permissions();
        if pending
            .iter()
            .any(|request| request["id"] == "held-question")
        {
            answered = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    assert!(
        answered,
        "the question raised while the control waited never reached the host"
    );
    // Answering it here releases the withheld native answer.
    session
        .respond_permission(
            "held-question",
            &json!({"behavior":"allow","updatedInput":{"answers":{"value":"A"}}}),
        )
        .await
        .unwrap();
    assert!(session.pending_permissions().is_empty());
    assert_eq!(
        control(session.as_mut(), &receipt.token).await,
        ChildControlOutcome::Stop(Ok(ChildStopOutcome::Cancelled))
    );
    // The parent turn still settles normally after the control exchange.
    session.start_turn("hello", &spec.config).await.unwrap();
    complete(session.as_mut()).await;
    session.close().await.unwrap();
}

#[tokio::test]
async fn closed_sessions_reject_child_controls() {
    let (_root, mut session, _spec) = subagents().await;
    session.close().await.unwrap();
    assert!(
        session
            .guide_child("worker", "late")
            .await
            .is_err_and(|error| error == AgentSessionError::Rejected)
    );
    assert!(
        session
            .stop_child("worker")
            .await
            .is_err_and(|error| error == AgentSessionError::Rejected)
    );
}

/// Open one parent Agent in a real provider worker whose native peer is the OMP fixture.
async fn worker() -> (
    tempfile::TempDir,
    crate::service::agent_execution::AgentExecution,
    String,
) {
    let files = crate::test_support::Fixture::new();
    let agent_dir = files.root.path().join("agent");
    let client = OmpClient {
        environment: AgentEnvironment::try_from(BTreeMap::from([(
            "PI_CODING_AGENT_DIR".to_owned(),
            agent_dir.to_str().unwrap().to_owned(),
        )]))
        .unwrap(),
        ..OmpClient::new(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src/local/omp/tests/fixtures/omp.py"),
        )
    };
    let (execution, _registry) = crate::test_support::worker_with_client(
        &files,
        model::creation::Creations::default(),
        Box::new(client),
    );
    let created = execution
        .execute(
            "agent.create.request",
            json!({"config":{"provider":"omp","cwd":files.cwd.to_str().unwrap()}}),
        )
        .await
        .unwrap();
    let parent = created["agentId"].as_str().unwrap().to_owned();
    (files.root, execution, parent)
}

/// Poll until the parent reports its running observed child.
async fn child_id(
    execution: &crate::service::agent_execution::AgentExecution,
    parent: &str,
) -> String {
    let listing = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let listed = execution
                .execute(
                    "agent.subagents.list.request",
                    json!({"parentAgentId":parent}),
                )
                .await
                .unwrap();
            if let Some(child) = listed["agents"]
                .as_array()
                .and_then(|agents| agents.first())
                .filter(|child| child["observation"]["status"] == "running")
            {
                return child["id"].as_str().unwrap().to_owned();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    listing.expect("the parent never published a running child")
}

#[tokio::test]
async fn a_permission_question_is_answerable_through_the_worker_while_a_control_waits() {
    let (root, execution, parent) = worker().await;
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":parent,"text":"subagent-controls"}),
        )
        .await
        .unwrap();
    let child = child_id(&execution, &parent).await;
    // The native peer withholds the cancel answer until its own question is answered, so the
    // control receipt stays pending until this RPC is serviced on the same parent lane.
    std::fs::write(root.path().join("agent/hold-control-answer"), "").unwrap();
    let control_writer = execution.clone();
    let mut stopping = Box::pin(control_writer.execute(
        "agent.subagents.stop.request",
        json!({"parentAgentId":parent,"childAgentId":child}),
    ));
    let resolve = json!({"agentId":parent,"requestId":"held-question",
        "response":{"behavior":"allow","updatedInput":{"answers":{"value":"A"}}}});
    let resolved = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            // The control must still be waiting; only the question may settle first.
            if let Ok(result) = tokio::time::timeout(Duration::ZERO, &mut stopping).await {
                panic!("the child control settled before its question was answered: {result:?}");
            }
            if execution
                .execute("agent.permission.resolve.request", resolve.clone())
                .await
                .is_ok()
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    resolved.expect("a question raised during a pending child control was never answerable");
    let receipt = tokio::time::timeout(Duration::from_secs(10), stopping)
        .await
        .expect("the parked child control never answered")
        .unwrap();
    assert_eq!(receipt["disposition"], "cancelled");
    assert_eq!(receipt["childAgentId"], json!(child));
    execution.shutdown().await.unwrap();
}
