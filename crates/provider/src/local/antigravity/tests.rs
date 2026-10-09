use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;
use crate::ports::agent_session::AgentTurnEvent;
use crate::protocol::prompt::{AgentPrompt, PromptImage};

fn fixture() -> (TempDir, AntigravityClient, AgentSessionSpec) {
    let directory = tempfile::tempdir().unwrap();
    let cwd = directory.path().canonicalize().unwrap();
    let program = cwd.join("agy");
    std::fs::write(&program, include_str!("tests/fixtures/agy.py")).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut client = AntigravityClient::new(program);
    client.environment = AgentEnvironment::try_from(BTreeMap::from([(
        "AGY_FIXTURE_LOG".to_owned(),
        cwd.join("requests.jsonl").to_str().unwrap().to_owned(),
    )]))
    .unwrap();
    let spec = AgentSessionSpec {
        provider: PROVIDER.to_owned(),
        cwd: cwd.to_str().unwrap().to_owned(),
        config: StoredAgentConfig::default(),
    };
    (directory, client, spec)
}

fn logs(directory: &TempDir) -> Vec<Value> {
    std::fs::read_to_string(directory.path().join("requests.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

async fn event(session: &mut dyn AgentSession) -> Result<AgentTurnEvent, AgentSessionError> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(event) = session.poll_turn()? {
                return Ok(event);
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}

async fn completed(session: &mut dyn AgentSession) -> Vec<AgentTurnEvent> {
    let mut events = Vec::new();
    loop {
        let next = event(session).await.unwrap();
        let terminal = matches!(next, AgentTurnEvent::Completed(_));
        events.push(next);
        if terminal {
            return events;
        }
    }
}

#[tokio::test]
async fn discovers_native_models_and_labels_without_a_prompt() {
    let (directory, client, spec) = fixture();
    assert!(client.is_available().await.unwrap());
    assert!(!client.supports_history_replay());
    let details = client.discover(&spec.cwd).await.unwrap();
    assert_eq!(details.models.len(), 2);
    assert_eq!(details.models[0]["id"], "gemini-test-low");
    assert_eq!(details.modes.len(), 4);
    assert_eq!(
        client.settings(&spec.config)["capabilities"]["supportsMcpServers"],
        false
    );
    assert!(client.diagnostic().await.unwrap().contains("2 models"));
    assert!(
        logs(&directory)
            .iter()
            .all(|entry| entry.get("input").is_none())
    );
    let mut manager = crate::service::agent_manager::AgentManager::new(Box::new(
        persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry::new(
            directory.path().join("agents.json"),
        ),
    ));
    manager.register_client(Box::new(client)).unwrap();
    let snapshot = manager
        .providers("provider.snapshot.get.request", json!({"cwd":spec.cwd}))
        .await
        .unwrap();
    assert_eq!(snapshot["entries"][0]["label"], "Antigravity");
    assert_eq!(snapshot["entries"][0]["status"], "ready");
    assert_eq!(snapshot["entries"][0]["defaultModeId"], "default");
}

#[tokio::test]
async fn streams_multiple_turns_resumes_and_restarts_for_config_changes() {
    let (directory, client, mut spec) = fixture();
    spec.config.model = Some("gemini-test-low".to_owned());
    client.validate_selection(&spec).await.unwrap();
    let mut session = client.create_session(&spec).await.unwrap();
    assert_eq!(
        session.runtime_info().await.unwrap().model,
        spec.config.model
    );
    let handle = session.persistence().unwrap();
    let prompt = AgentPrompt {
        text: "hello".to_owned(),
        client_message_id: Some("client-id".to_owned()),
        ..AgentPrompt::default()
    };
    session.start_input(&prompt, &spec.config).await.unwrap();
    assert!(session.start_turn("busy", &spec.config).await.is_err());
    let first = completed(session.as_mut()).await;
    let timeline: Vec<_> = first
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Timeline(entry) => Some(entry),
            _ => None,
        })
        .collect();
    assert_eq!(timeline.len(), 2);
    assert_eq!(timeline[0].item["type"], "tool_call");
    assert_eq!(timeline[1].item["text"], "Hello world");
    session.start_turn("second", &spec.config).await.unwrap();
    let second = completed(session.as_mut()).await;
    assert!(
        second
            .iter()
            .any(|event| matches!(event, AgentTurnEvent::Usage(usage)
        if usage.input_tokens == Some(200)))
    );
    spec.config.mode_id = Some("full-access".to_owned());
    spec.config.model = Some("claude-test-high".to_owned());
    session.start_turn("third", &spec.config).await.unwrap();
    completed(session.as_mut()).await;
    assert_eq!(session.persistence().unwrap(), handle);
    session.close().await.unwrap();
    session.close().await.unwrap();
    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    resumed.start_turn("resumed", &spec.config).await.unwrap();
    completed(resumed.as_mut()).await;
    resumed.close().await.unwrap();
    let calls = logs(&directory);
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.get("input").is_some())
            .count(),
        4
    );
    assert!(
        calls
            .iter()
            .filter_map(|call| call["args"].as_array())
            .any(|args| args
                .iter()
                .any(|arg| arg == "--dangerously-skip-permissions")
                && args.iter().any(|arg| arg == &handle.session_id))
    );
}

#[tokio::test]
async fn rejects_foreign_handles_models_and_unsupported_images_before_admission() {
    let (directory, client, mut spec) = fixture();
    spec.config.model = Some("missing-model".to_owned());
    assert_eq!(
        client.validate_selection(&spec).await,
        Err(AgentSessionError::Rejected)
    );
    spec.config.model = None;
    let mut session = client.create_session(&spec).await.unwrap();
    let handle = session.persistence().unwrap();
    assert!(
        client
            .resume_session(&handle, &spec, AgentResumePurpose::History)
            .await
            .is_err()
    );
    for bad in [
        AgentPersistenceHandle {
            provider: "codex".to_owned(),
            ..handle.clone()
        },
        AgentPersistenceHandle {
            session_id: "invalid".to_owned(),
            ..handle.clone()
        },
        AgentPersistenceHandle {
            metadata: None,
            ..handle.clone()
        },
    ] {
        assert!(
            client
                .resume_session(&bad, &spec, AgentResumePurpose::Interactive)
                .await
                .is_err()
        );
    }
    let image = AgentPrompt {
        text: "image".to_owned(),
        images: vec![PromptImage {
            data: "aGVsbG8=".to_owned(),
            mime_type: "image/png".to_owned(),
        }],
        ..Default::default()
    };
    assert!(session.start_input(&image, &spec.config).await.is_err());
    let schema = AgentPrompt {
        text: "schema".to_owned(),
        output_schema: Some(json!({})),
        ..Default::default()
    };
    assert!(session.start_input(&schema, &spec.config).await.is_err());
    session.close().await.unwrap();
    assert!(
        logs(&directory)
            .iter()
            .all(|entry| entry.get("input").is_none())
    );
}

#[tokio::test]
async fn cancellation_waits_for_native_acknowledgement_and_allows_a_later_turn() {
    let (_directory, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    assert!(session.cancel_turn("foreign-turn").await.is_err());
    session.cancel_turn(&turn).await.unwrap();
    loop {
        if event(session.as_mut()).await.unwrap() == AgentTurnEvent::Cancelled {
            break;
        }
    }
    session
        .start_turn("after cancel", &spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    session.close().await.unwrap();
}

#[tokio::test]
async fn ephemeral_environment_never_enters_the_resume_handle() {
    let (directory, client, spec) = fixture();
    let environment = AgentEnvironment::try_from(BTreeMap::from([
        (
            "AGY_FIXTURE_LOG".to_owned(),
            directory
                .path()
                .join("requests.jsonl")
                .to_str()
                .unwrap()
                .to_owned(),
        ),
        ("AGY_TEST_ENV".to_owned(), "test-only-value".to_owned()),
    ]))
    .unwrap();
    let mut session = client
        .create_session_with_environment(&spec, &environment)
        .await
        .unwrap();
    assert!(
        !serde_json::to_string(&session.persistence())
            .unwrap()
            .contains("test-only-value")
    );
    session.close().await.unwrap();
    assert!(
        logs(&directory)
            .iter()
            .all(|call| call["hasEnvironment"] == true)
    );
}

#[tokio::test]
async fn startup_errors_and_timeout_do_not_hang() {
    for scenario in ["hung", "malformed", "oversized", "wrong-model"] {
        let (_directory, mut client, mut spec) = fixture();
        client.deadline = Duration::from_millis(300);
        client.environment = AgentEnvironment::try_from(BTreeMap::from([(
            "AGY_FIXTURE_SCENARIO".to_owned(),
            scenario.to_owned(),
        )]))
        .unwrap();
        spec.config.model = Some("gemini-test-low".to_owned());
        assert!(client.create_session(&spec).await.is_err(), "{scenario}");
    }
}

#[tokio::test]
async fn crashes_foreign_events_and_error_results_do_not_complete_successfully() {
    for scenario in ["crash", "foreign", "failure"] {
        let (_directory, mut client, spec) = fixture();
        client.environment = AgentEnvironment::try_from(BTreeMap::from([(
            "AGY_FIXTURE_SCENARIO".to_owned(),
            scenario.to_owned(),
        )]))
        .unwrap();
        let mut session = client.create_session(&spec).await.unwrap();
        session.start_turn("hello", &spec.config).await.unwrap();
        loop {
            match event(session.as_mut()).await {
                Ok(AgentTurnEvent::RuntimeInfo(_) | AgentTurnEvent::Usage(_)) => {}
                Ok(AgentTurnEvent::Failed) | Err(AgentSessionError::Failed) => break,
                unexpected => panic!("unexpected {unexpected:?}"),
            }
        }
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn missing_explicit_binary_is_unavailable_without_installation_fallback() {
    let (_directory, _client, spec) = fixture();
    let client = AntigravityClient::new(PathBuf::from(&spec.cwd).join("missing-agy"));
    assert!(!client.is_available().await.unwrap());
    assert!(client.create_session(&spec).await.is_err());
    assert!(client.diagnostic().await.unwrap().contains("unavailable"));
    // Exercise installed discovery without launching or reading any credentials.
    assert!(
        !AntigravityClient::installed()
            .program
            .as_os_str()
            .is_empty()
    );
}

#[tokio::test]
async fn manager_persists_timeline_and_resume_identity_across_restart() {
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

    use crate::service::agent_manager::{AgentManager, AgentRegistration};
    use crate::storage::timeline::Timeline;
    let (directory, client, spec) = fixture();
    let registry = FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json"));
    let timeline = Timeline::memory().unwrap();
    let mut manager = AgentManager::new(Box::new(registry.clone())).with_timeline(timeline.clone());
    manager.register_client(Box::new(client.clone())).unwrap();
    let initial = manager
        .create("agy-agent", &spec, AgentRegistration::default())
        .await
        .unwrap();
    manager.send("agy-agent", "hello").await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while manager.active_turn("agy-agent").is_some() {
            manager.poll().await.unwrap();
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(manager.last_message("agy-agent"), Some("Hello world"));
    let (_, entries) = timeline.read("agy-agent").unwrap();
    assert!(
        entries
            .iter()
            .any(|row| row.entry.item["type"] == "tool_call")
    );
    let text: String = entries
        .iter()
        .filter(|row| row.entry.item["type"] == "assistant_message")
        .filter_map(|row| row.entry.item["text"].as_str())
        .collect();
    assert_eq!(text, "Hello world");
    manager.close_all().await.unwrap();
    let mut restored = AgentManager::new(Box::new(registry)).with_timeline(timeline.clone());
    restored.register_client(Box::new(client)).unwrap();
    let resumed = restored
        .restore("agy-agent", &crate::protocol::resume::Overrides::default())
        .await
        .unwrap();
    assert_eq!(resumed.persistence, initial.persistence);
    let original: Vec<_> = entries.into_iter().map(|row| row.entry).collect();
    let retained: Vec<_> = timeline
        .read("agy-agent")
        .unwrap()
        .1
        .into_iter()
        .map(|row| row.entry)
        .collect();
    assert_eq!(retained, original);
    restored.send("agy-agent", "after restart").await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while restored.active_turn("agy-agent").is_some() {
            restored.poll().await.unwrap();
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let text: String = timeline
        .read("agy-agent")
        .unwrap()
        .1
        .iter()
        .filter(|row| row.entry.item["type"] == "assistant_message")
        .filter_map(|row| row.entry.item["text"].as_str())
        .collect();
    assert_eq!(text, "Hello worldHello world");
    restored.close_all().await.unwrap();
}

#[cfg(test)]
mod installed;

mod diagnostics;
