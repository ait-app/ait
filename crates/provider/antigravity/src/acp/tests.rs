use std::collections::BTreeMap;

use provider_core::ports::{
    agent_session::{AgentClient, AgentResumePurpose},
    environment::AgentEnvironment,
};
use tempfile::TempDir;

use super::*;
use crate::tests::logs;

mod audit;
mod compatibility;
mod native;
mod recovery;

fn fixture(scenario: &str) -> (TempDir, AntigravityClient, AgentSessionSpec) {
    let (directory, mut client, spec) = crate::tests::fixture();
    client.program = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/fixtures/acp.py");
    client.environment = AgentEnvironment::try_from(BTreeMap::from([
        (
            "AGY_FIXTURE_LOG".to_owned(),
            directory
                .path()
                .join("requests.jsonl")
                .to_str()
                .unwrap()
                .to_owned(),
        ),
        ("AGY_ACP_SCENARIO".to_owned(), scenario.to_owned()),
    ]))
    .unwrap();
    (directory, client, spec)
}

async fn next(session: &mut Session) -> AgentTurnEvent {
    if let Some(event) = session.poll_turn().unwrap() {
        return event;
    }
    let event = tokio::time::timeout(
        Duration::from_secs(120),
        session.running.as_mut().unwrap().events.recv(),
    )
    .await
    .unwrap()
    .unwrap();
    session.observe(event)
}

async fn finish(session: &mut Session) -> Vec<AgentTurnEvent> {
    let mut events = Vec::new();
    loop {
        let event = next(session).await;
        let done = turn::terminal(&event);
        events.push(event);
        if done {
            return events;
        }
    }
}

#[tokio::test]
async fn marked_handles_resume_without_replaying_prompts_and_configuration_is_confirmed() {
    let (directory, client, mut spec) = fixture("");
    spec.config.model = Some("gemini-test-low".to_owned());
    let mut session = open(&client, &spec, None).await.unwrap();
    let handle = session.persistence().unwrap();
    assert_eq!(handle.metadata.as_ref().unwrap()["transport"], "acp");
    assert_eq!(session.runtime().model.as_deref(), Some("gemini-test"));
    assert_eq!(session.runtime().thinking_option_id.as_deref(), Some("low"));
    for text in ["first", "second"] {
        session.start_turn(text, &spec.config).await.unwrap();
        let events = finish(&mut session).await;
        assert_eq!(
            events.last(),
            Some(&AgentTurnEvent::Completed(Some(text.to_owned())))
        );
        let usage: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                AgentTurnEvent::RequestUsage(usage) => Some(usage),
                _ => None,
            })
            .collect();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].input_tokens, 10);
        assert_eq!(usage[0].cache_read_tokens, 5);
        assert!(events.iter().any(|event|matches!(event,AgentTurnEvent::Usage(usage) if usage.context_window_used_tokens == Some(42) && usage.context_window_max_tokens == Some(1000))));
    }
    let handle = session.persistence().unwrap();
    assert_eq!(
        handle.metadata.as_ref().unwrap()["acpUsage"]["inputTokens"],
        20
    );
    session.close().await.unwrap();
    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert_eq!(resumed.persistence().unwrap(), handle);
    resumed.close().await.unwrap();
    let frames = logs(&directory);
    assert_eq!(
        frames
            .iter()
            .filter(|frame| frame["rpc"]["method"] == "session/prompt")
            .count(),
        2
    );
    assert!(
        frames
            .iter()
            .any(|frame| frame["rpc"]["method"] == "session/resume"
                && frame["rpc"]["params"]["sessionId"] == handle.session_id)
    );
    assert!(
        frames
            .iter()
            .filter(|frame| frame["rpc"]["method"] == "initialize")
            .all(|frame| frame["rpc"]["params"]["clientInfo"]["name"] == "ait")
    );
}

#[tokio::test]
async fn missing_capabilities_and_errors_are_rejected_without_headless_fallback() {
    for scenario in [
        "method-missing",
        "missing-option",
        "auth",
        "quota",
        "network",
        "unknown-error",
    ] {
        let (directory, client, spec) = fixture(scenario);
        assert!(client.create_session(&spec).await.is_err(), "{scenario}");
        assert!(logs(&directory).iter().all(|frame| {
            frame["args"]
                .as_array()
                .is_none_or(|args| !args.contains(&json!("--input-format")))
        }));
    }
}

#[tokio::test]
async fn controls_reject_wrong_resume_and_unacknowledged_configuration() {
    let (_directory, client, spec) = fixture("wrong-session");
    let handle =
        acp_session::persistence(PROVIDER, "11111111-2222-4333-8444-555555555555", &spec.cwd);
    assert!(open(&client, &spec, Some(&handle)).await.is_err());
    let (_directory, client, mut spec) = fixture("unacknowledged-config");
    spec.config.thinking_option_id = Some("low".to_owned());
    assert!(matches!(
        open(&client, &spec, None).await,
        Err(OpenError::Session(AgentSessionError::Failed))
    ));
}

#[tokio::test]
async fn modes_are_native_and_only_full_access_adds_the_skip_flag() {
    for mode in ["default", "accept-edits", "auto", "plan", "full-access"] {
        let (directory, client, mut spec) = fixture("");
        spec.config.mode_id = Some(mode.to_owned());
        let mut session = open(&client, &spec, None).await.unwrap();
        assert_eq!(session.runtime().mode_id.as_deref(), Some(mode));
        session.close().await.unwrap();
        let frames = logs(&directory);
        let args = frames
            .iter()
            .find(|frame| frame["args"][0] == "acp")
            .unwrap()["args"]
            .as_array()
            .unwrap();
        assert_eq!(
            args.contains(&json!("--dangerously-skip-permissions")),
            mode == "full-access"
        );
    }
}

#[tokio::test]
async fn malformed_partial_and_foreign_streams_fail_once_without_retry() {
    for text in [
        "disconnect",
        "malformed",
        "oversized",
        "stale",
        "foreign",
        "usage-invalid",
    ] {
        let (directory, client, spec) = fixture("");
        let mut session = open(&client, &spec, None).await.unwrap();
        session.start_turn(text, &spec.config).await.unwrap();
        let events = finish(&mut session).await;
        assert_eq!(events.last(), Some(&AgentTurnEvent::Failed), "{text}");
        assert!(session.poll_turn().unwrap().is_none());
        if text == "disconnect" {
            assert!(events.iter().any(|event|matches!(event,AgentTurnEvent::Timeline(item) if item.item["type"] == "tool_call" && item.item["status"] == "failed")));
        }
        session.start_turn("recover", &spec.config).await.unwrap();
        assert!(matches!(
            finish(&mut session).await.last(),
            Some(AgentTurnEvent::Completed(_))
        ));
        assert_eq!(
            logs(&directory)
                .iter()
                .filter(|frame| frame["rpc"]["method"] == "session/prompt")
                .count(),
            2
        );
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn callbacks_deny_unattended_requests_without_host_permissions() {
    let (_directory, client, spec) = fixture("");
    let mut session = open(&client, &spec, None).await.unwrap();
    session.start_turn("deny", &spec.config).await.unwrap();
    let events = finish(&mut session).await;
    assert!(
        events
            .iter()
            .all(|event| !matches!(event, AgentTurnEvent::PermissionRequested(_)))
    );
    assert!(events.iter().any(|event|matches!(event,AgentTurnEvent::Timeline(item) if item.item["status"] == "failed" && item.item["detail"]["type"] == "write")));
    assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
    session.close().await.unwrap();
}

#[tokio::test]
async fn cancel_and_complete_races_settle_once_and_recover_the_same_session() {
    for text in ["wait", "race"] {
        let (_directory, client, spec) = fixture("");
        let mut session = open(&client, &spec, None).await.unwrap();
        let handle = session.persistence().unwrap();
        let turn = session.start_turn(text, &spec.config).await.unwrap();
        loop {
            if matches!(next(&mut session).await,AgentTurnEvent::Progress{entry,..} if entry.item["type"] == "tool_call")
            {
                break;
            }
        }
        let result = session.cancel_turn(&turn).await;
        assert_eq!(result.is_ok(), text == "wait");
        let events = finish(&mut session).await;
        assert_eq!(
            events.iter().filter(|event| turn::terminal(event)).count(),
            1
        );
        assert!(session.poll_turn().unwrap().is_none());
        session
            .start_turn("after cancel", &spec.config)
            .await
            .unwrap();
        finish(&mut session).await;
        assert_eq!(session.persistence().unwrap().session_id, handle.session_id);
        session.close().await.unwrap();
    }
}

#[test]
fn usage_is_per_turn_with_thoughts_already_in_output() {
    let request = usage::request("one",&json!({"inputTokens":22660,"outputTokens":470,"cachedReadTokens":47177,"cachedWriteTokens":0,"thoughtTokens":33})).unwrap().unwrap();
    assert_eq!(request.output_tokens, 470);
    assert_eq!(request.reasoning_tokens, Some(33));
    assert_eq!(
        request.boundary,
        provider_core::ports::usage_ledger::UsageBoundary::Turn
    );
    assert!(usage::request("bad", &json!({"inputTokens":-1})).is_err());
    assert!(usage::request("bad", &json!({"outputTokens":3,"thoughtTokens":4})).is_err());
    assert!(usage::request("missing", &Value::Null).unwrap().is_none());
    for malformed in [json!("invalid"), json!([]), json!(true)] {
        assert!(usage::request("malformed", &malformed).is_err());
    }
}
