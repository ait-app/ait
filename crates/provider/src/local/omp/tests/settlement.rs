//! Native state checks recover missing settlement without guessing from assistant text.
use super::*;

fn calls(root: &tempfile::TempDir) -> Vec<Value> {
    std::fs::read_to_string(root.path().join("agent/calls.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

async fn wait_for_check(session: &mut dyn AgentSession, root: &tempfile::TempDir, initial: usize) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            assert!(!matches!(
                session.poll_turn().unwrap(),
                Some(
                    AgentTurnEvent::Completed(_)
                        | AgentTurnEvent::Cancelled
                        | AgentTurnEvent::Failed
                )
            ));
            if calls(root)
                .iter()
                .filter(|call| call["type"] == "get_state")
                .count()
                > initial
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn missing_settlement_notification_reaches_idle_and_allows_the_next_input() {
    use crate::service::agent_manager::{AgentManager, AgentRegistration};
    use domain::agent_runtime::{AgentRuntimeStatus, registry::AgentRuntimeRegistry};
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

    let (root, client, spec) = fixture();
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    let mut manager = AgentManager::new(Box::new(registry.clone()));
    manager.register_client(Box::new(client)).unwrap();
    manager
        .create("agent", &spec, AgentRegistration::default())
        .await
        .unwrap();
    for prompt in ["lost-settlement", "next"] {
        manager.send("agent", prompt).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while manager.active_turn("agent").is_some() {
                manager.poll().await.unwrap();
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            registry.get("agent").unwrap().unwrap().last_status,
            AgentRuntimeStatus::Idle
        );
    }
    assert_eq!(
        calls(&root)
            .iter()
            .filter(|call| call["type"] == "prompt")
            .count(),
        2
    );
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn an_idle_session_without_a_correlated_outcome_fails_without_resending() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("lost-result", &spec.config)
        .await
        .unwrap();
    assert_eq!(
        complete(session.as_mut()).await.last(),
        Some(&AgentTurnEvent::Failed)
    );
    assert!(
        session
            .failure_message()
            .unwrap()
            .contains("without reporting this prompt's outcome")
    );
    assert_eq!(
        calls(&root)
            .iter()
            .filter(|call| call["type"] == "prompt")
            .count(),
        1
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn background_work_stays_active_until_native_state_confirms_settlement() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let initial = calls(&root)
        .iter()
        .filter(|call| call["type"] == "get_state")
        .count();
    session
        .start_turn("busy-after-yield", &spec.config)
        .await
        .unwrap();
    wait_for_check(session.as_mut(), &root, initial).await;
    std::fs::write(root.path().join("agent/release-settlement"), "").unwrap();
    assert_eq!(
        complete(session.as_mut()).await.last(),
        Some(&AgentTurnEvent::Completed(Some(
            "Background finished".to_owned()
        )))
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn missing_native_settlement_facts_do_not_manufacture_completion() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let initial = calls(&root)
        .iter()
        .filter(|call| call["type"] == "get_state")
        .count();
    let turn = session
        .start_turn("missing-settlement-fact", &spec.config)
        .await
        .unwrap();
    wait_for_check(session.as_mut(), &root, initial).await;
    session.cancel_turn(&turn).await.unwrap();
    assert_eq!(
        complete(session.as_mut()).await.last(),
        Some(&AgentTurnEvent::Cancelled)
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn cancellation_returns_before_native_cleanup_and_is_submitted_only_once() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let turn = session
        .start_turn("wait-slow-abort", &spec.config)
        .await
        .unwrap();
    next(session.as_mut()).await.unwrap();
    for _ in 0..2 {
        tokio::time::timeout(Duration::from_millis(500), session.cancel_turn(&turn))
            .await
            .expect("cancel must not wait for the native abort response")
            .unwrap();
    }
    std::fs::write(root.path().join("agent/release-abort"), "").unwrap();
    assert_eq!(
        complete(session.as_mut()).await.last(),
        Some(&AgentTurnEvent::Cancelled)
    );
    assert_eq!(
        calls(&root)
            .iter()
            .filter(|call| call["type"] == "abort_and_restore_queue")
            .count(),
        1
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn cancellation_with_a_missing_abort_acknowledgement_uses_native_settlement() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let turn = session
        .start_turn("wait-lost-abort", &spec.config)
        .await
        .unwrap();
    next(session.as_mut()).await.unwrap();
    tokio::time::timeout(Duration::from_millis(500), session.cancel_turn(&turn))
        .await
        .expect("cancel must not wait for a lost native response")
        .unwrap();
    assert_eq!(
        complete(session.as_mut()).await.last(),
        Some(&AgentTurnEvent::Cancelled)
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn manager_cancellation_clears_running_even_when_native_abort_confirmation_is_missing() {
    use crate::service::agent_manager::{AgentManager, AgentRegistration};
    use crate::storage::timeline::Timeline;
    use domain::agent_runtime::{AgentRuntimeStatus, registry::AgentRuntimeRegistry};
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

    for (prompt, expected) in [
        ("wait-lost-abort", AgentRuntimeStatus::Idle),
        ("wait-stuck-abort", AgentRuntimeStatus::Error),
    ] {
        let (root, client, spec) = fixture();
        let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
        let timeline = Timeline::memory().unwrap();
        let mut manager =
            AgentManager::new(Box::new(registry.clone())).with_timeline(timeline.clone());
        manager.register_client(Box::new(client)).unwrap();
        manager
            .create("agent", &spec, AgentRegistration::default())
            .await
            .unwrap();
        manager.send("agent", prompt).await.unwrap();
        tokio::time::timeout(Duration::from_millis(500), manager.cancel("agent"))
            .await
            .expect("the stop request must not wait for native cleanup")
            .unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            while manager.active_turn("agent").is_some() {
                manager.poll().await.unwrap();
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
        let record = registry.get("agent").unwrap().unwrap();
        assert_eq!(record.last_status, expected, "{prompt}");
        if expected == AgentRuntimeStatus::Error {
            let reason = record.last_error.unwrap();
            assert!(reason.contains("did not confirm cancellation"));
            assert!(
                timeline
                    .read("agent")
                    .unwrap()
                    .1
                    .iter()
                    .any(|row| row.entry.item["type"] == "error"
                        && row.entry.item["message"] == reason)
            );
        }
        manager.send("agent", "next").await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while manager.active_turn("agent").is_some() {
                manager.poll().await.unwrap();
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            registry.get("agent").unwrap().unwrap().last_status,
            AgentRuntimeStatus::Idle
        );
        assert_eq!(
            calls(&root)
                .iter()
                .filter(|call| call["type"] == "prompt")
                .count(),
            2
        );
        manager.close_all().await.unwrap();
    }
}
