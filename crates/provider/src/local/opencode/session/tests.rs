//! Stop admission must remain independent of output backpressure and final history replay.
use std::time::Duration;

use super::*;
use crate::local::opencode::tests::{event, fixture, requests};

#[tokio::test]
async fn delayed_host_polling_keeps_an_already_settled_native_outcome() {
    let (_root, client, spec) = fixture("normal");
    let mut session = open(&client, &spec, None).await.unwrap();
    let turn = session.start_turn("work", &spec.config).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !session.task.as_ref().unwrap().is_finished() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    session.cancel_turn(&turn).await.unwrap();
    session
        .cancellation
        .as_ref()
        .unwrap()
        .send_replace(Some(Instant::now() - CANCEL_GRACE));

    loop {
        match event(&mut session).await {
            AgentTurnEvent::Completed(_) => break,
            AgentTurnEvent::Cancelled | AgentTurnEvent::Failed => panic!("settled turn replaced"),
            _ => {}
        }
    }
    assert!(session.failure_message().is_none());
    session.close().await.unwrap();
}

#[tokio::test]
async fn cancellation_returns_while_output_is_backpressured_and_is_sent_only_once() {
    let (root, client, spec) = fixture("cancel-backpressure");
    let mut session = open(&client, &spec, None).await.unwrap();
    let turn = session.start_turn("work", &spec.config).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while session.events.capacity() != 0 {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();

    for _ in 0..2 {
        tokio::time::timeout(Duration::from_millis(500), session.cancel_turn(&turn))
            .await
            .expect("stop must not wait for an executor blocked on host output")
            .unwrap();
    }

    loop {
        match event(&mut session).await {
            AgentTurnEvent::Cancelled => break,
            AgentTurnEvent::Completed(_) | AgentTurnEvent::Failed => panic!("incorrect stop"),
            _ => {}
        }
    }
    assert_eq!(
        requests(&root)
            .iter()
            .filter(|request| request["method"] == "session/cancel")
            .count(),
        1
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn cancellation_during_final_replay_returns_without_rewriting_the_native_outcome() {
    let (root, client, spec) = fixture("cancel-during-replay");
    let mut session = open(&client, &spec, None).await.unwrap();
    let turn = session.start_turn("work", &spec.config).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !root.path().join("replay.entered").exists() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();

    tokio::time::timeout(Duration::from_millis(500), session.cancel_turn(&turn))
        .await
        .expect("stop must not wait for native history replay")
        .unwrap();
    std::fs::write(root.path().join("replay.release"), "").unwrap();

    for prompt in ["work", "Continue"] {
        if prompt == "Continue" {
            session.start_turn(prompt, &spec.config).await.unwrap();
        }
        loop {
            match event(&mut session).await {
                AgentTurnEvent::Completed(text) => {
                    assert_eq!(text.as_deref(), Some("authoritative answer"));
                    break;
                }
                AgentTurnEvent::Cancelled | AgentTurnEvent::Failed => {
                    panic!("native stop replaced")
                }
                _ => {}
            }
        }
    }
    assert!(
        !requests(&root)
            .iter()
            .any(|request| request["method"] == "session/cancel")
    );
    session.close().await.unwrap();
}
