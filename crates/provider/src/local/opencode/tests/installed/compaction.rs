//! Exercise native automatic compaction using only an isolated loopback model.
use super::*;

const SEED: &str = "Automatic compaction seed";
const SUMMARY: &str = "Automatic compaction summary sentinel";

#[tokio::test]
#[ignore = "requires OpenCode 1.x in AIT_TEST_OPENCODE_BIN; isolated XDG and loopback model"]
async fn installed_acp_automatic_compaction_keeps_the_session_usable() {
    let mut fixture = installed(false).await;
    assert!(
        launcher::version(&fixture.client, &fixture.spec.cwd)
            .await
            .unwrap()
            .starts_with("1.")
    );
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    let mut history = Vec::new();
    for prompt in [SEED, "Continue after automatic compaction"] {
        let turn = session
            .start_turn(prompt, &fixture.spec.config)
            .await
            .unwrap();
        let mut states = Vec::new();
        loop {
            match event(session.as_mut()).await {
                AgentTurnEvent::Timeline(entry) if entry.item["type"] == "compaction" => {
                    states.push(entry.item["status"].as_str().unwrap().to_owned());
                }
                AgentTurnEvent::History(entries) => history = entries,
                AgentTurnEvent::Completed(_) => break,
                AgentTurnEvent::Failed | AgentTurnEvent::Cancelled => {
                    panic!("native compaction failed")
                }
                _ => {}
            }
        }
        if prompt == SEED {
            assert_eq!(states, ["loading", "completed"]);
            assert!(
                history
                    .iter()
                    .any(|entry| entry.item["type"] == "compaction"
                        && entry.item["status"] == "completed")
            );
            assert!(
                history
                    .iter()
                    .filter(|entry| entry.item["type"] == "user_message")
                    .count()
                    > 1
            );
            assert!(
                history
                    .iter()
                    .all(|entry| entry.turn_id.as_deref() == Some(&turn))
            );
        }
    }
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    assert_replay(
        &history,
        &fixture
            .client
            .history(&handle, &fixture.spec.cwd)
            .await
            .unwrap(),
    );
    let mut requests = Vec::new();
    while let Ok(request) = fixture.requests.try_recv() {
        requests.push(request);
    }
    assert!(
        requests
            .iter()
            .any(|request| request["messages"].to_string().contains(SUMMARY)),
        "a subsequent model request must contain the native automatic summary"
    );
    let mut resumed = fixture
        .client
        .resume_session(&handle, &fixture.spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    resumed
        .start_turn("Continue after restart", &fixture.spec.config)
        .await
        .unwrap();
    completed(resumed.as_mut()).await;
    resumed.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires OpenCode 1.18.4; native compaction-only input identity and ACP history replay"]
async fn installed_acp_manual_compaction_keeps_its_accepted_turn_after_replay() {
    let fixture = installed(false).await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn("Seed manual compaction", &fixture.spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    let turn = session
        .start_turn("/compact", &fixture.spec.config)
        .await
        .unwrap();
    let mut history = Vec::new();
    loop {
        match event(session.as_mut()).await {
            AgentTurnEvent::History(entries) => history = entries,
            AgentTurnEvent::Completed(_) => break,
            AgentTurnEvent::Failed | AgentTurnEvent::Cancelled => {
                panic!("manual compaction failed")
            }
            _ => {}
        }
    }
    let marker = history
        .iter()
        .find(|entry| entry.item["type"] == "compaction")
        .unwrap();
    assert_eq!(marker.item["status"], "completed");
    assert_eq!(marker.turn_id.as_deref(), Some(turn.as_str()));
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    assert_replay(
        &history,
        &fixture
            .client
            .history(&handle, &fixture.spec.cwd)
            .await
            .unwrap(),
    );
}

/// Force native overflow once; subsequent requests receive ordinary small usage.
pub(super) fn model_response(request: &Value, state: &Model) -> Option<Response> {
    if !request["messages"].to_string().contains(SEED) {
        return None;
    }
    let has_tools = request["tools"]
        .as_array()
        .is_some_and(|tools| !tools.is_empty());
    if !has_tools {
        return Some(streamed_response(
            &json!({"role":"assistant","content":SUMMARY}),
            false,
        ));
    }
    if is_user_message(request, SEED) && state.calls.fetch_add(1, Ordering::SeqCst) == 1 {
        return Some(streamed_response_with_usage(
            &json!({"role":"assistant","content":"authoritative answer"}),
            false,
            31_000,
        ));
    }
    None
}
