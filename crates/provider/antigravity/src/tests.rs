use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;
use provider_core::ports::agent_session::AgentTurnEvent;
use provider_core::protocol::prompt::{AgentPrompt, PromptImage};

mod authentication;
mod image_input;
mod input_history;
#[cfg(test)]
mod installed;
mod quota;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod sandbox;

pub(crate) fn fixture() -> (TempDir, AntigravityClient, AgentSessionSpec) {
    let directory = tempfile::tempdir().unwrap();
    let cwd = directory.path().canonicalize().unwrap();
    // An immutable executable avoids ETXTBSY when concurrent forks inherit writable handles.
    let program =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/fixtures/acp.py");
    let mut client = AntigravityClient::new(program).with_image_directory(cwd.join("images"));
    client.environment = AgentEnvironment::try_from(BTreeMap::from([(
        "AGY_FIXTURE_LOG".to_owned(),
        cwd.join("requests.jsonl").to_str().unwrap().to_owned(),
    )]))
    .unwrap();
    let spec = AgentSessionSpec {
        provider: PROVIDER.to_owned(),
        cwd: cwd.to_str().unwrap().to_owned(),
        config: StoredAgentConfig::default(),
        confinement: None,
    };
    (directory, client, spec)
}

pub(crate) fn logs(directory: &TempDir) -> Vec<Value> {
    std::fs::read_to_string(directory.path().join("requests.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

pub(crate) async fn event(
    session: &mut dyn AgentSession,
) -> Result<AgentTurnEvent, AgentSessionError> {
    event_with_deadline(session, Duration::from_secs(5)).await
}

pub(crate) async fn event_with_deadline(
    session: &mut dyn AgentSession,
    deadline: Duration,
) -> Result<AgentTurnEvent, AgentSessionError> {
    tokio::time::timeout(deadline, async {
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

pub(crate) async fn completed(session: &mut dyn AgentSession) -> Vec<AgentTurnEvent> {
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
    assert!(client.supports_write_confinement());
    assert!(!client.supports_history_replay());
    let details = client.discover(&spec.cwd).await.unwrap();
    assert_eq!(details.models.len(), 3);
    assert_eq!(details.models[0]["id"], "gemini-test");
    assert_eq!(details.models[0]["label"], "Gemini Test");
    assert_eq!(details.models[0]["defaultThinkingOptionId"], "high");
    assert_eq!(
        details.models[0]["thinkingOptions"],
        json!([{"id":"low","label":"Low"},{"id":"high","label":"High"},
            {"id":"xhigh","label":"XHigh"}])
    );
    assert_eq!(details.modes.len(), 4);
    assert_eq!(
        client.settings(&spec.config)["capabilities"]["supportsMcpServers"],
        false
    );
    assert!(client.diagnostic().await.unwrap().contains("3 models"));
    assert!(
        logs(&directory)
            .iter()
            .all(|entry| entry["rpc"]["method"] != "session/prompt")
    );
    let mut manager = provider::testing::Manager::new(
        provider::service::agent_manager::AgentManager::new(Box::new(
            persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry::new(
                directory.path().join("agents.json"),
            ),
        )),
    );
    manager.register(Box::new(client)).unwrap();
    let snapshot = manager
        .providers("provider.snapshot.get.request", json!({"cwd":spec.cwd}))
        .await
        .unwrap();
    assert_eq!(snapshot["entries"][0]["label"], "Antigravity");
    assert_eq!(snapshot["entries"][0]["status"], "ready");
    assert_eq!(snapshot["entries"][0]["defaultModeId"], "default");
}

#[tokio::test]
async fn rejects_a_thinking_level_the_model_does_not_offer() {
    let (directory, client, mut spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    spec.config.model = Some("gemini-test".to_owned());
    spec.config.thinking_option_id = Some("medium".to_owned());
    assert_eq!(
        client.validate_selection(&spec).await,
        Err(AgentSessionError::Rejected)
    );
    assert_eq!(
        session.start_turn("hello", &spec.config).await,
        Err(AgentSessionError::Rejected)
    );
    assert!(
        session
            .failure_message()
            .is_some_and(|message| message.contains("thinking level"))
    );
    assert!(
        logs(&directory)
            .iter()
            .all(|call| call["rpc"]["method"] != "session/prompt")
    );
    spec.config.thinking_option_id = Some("low".to_owned());
    session.start_turn("hello", &spec.config).await.unwrap();
    completed(session.as_mut()).await;
    assert!(session.failure_message().is_none());
    session.close().await.unwrap();
}

#[tokio::test]
async fn rejects_foreign_handles_models_and_invalid_images_before_admission() {
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
        AgentPersistenceHandle {
            metadata: Some(BTreeMap::from([
                ("cwd".to_owned(), json!(spec.cwd)),
                ("transport".to_owned(), json!("unknown")),
            ])),
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
            data: "invalid base64".to_owned(),
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
            .all(|entry| entry["rpc"]["method"] != "session/prompt")
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
            .filter(|call| call.get("args").is_some())
            .all(|call| call["hasEnvironment"] == true)
    );
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

    use provider::service::agent_manager::AgentManager;
    use provider::testing::Manager;
    let (directory, client, spec) = fixture();
    let registry = FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json"));
    let timeline = provider::testing::memory_timeline().unwrap();
    let mut manager =
        Manager::new(AgentManager::new(Box::new(registry.clone())).with_timeline(timeline.clone()));
    manager.register(Box::new(client.clone())).unwrap();
    let initial = manager.create("agy-agent", &spec).await.unwrap();
    manager.send("agy-agent", "tool").await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while manager.active_turn("agy-agent").is_some() {
            manager.poll().await.unwrap();
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(manager.last_message("agy-agent"), Some("tool"));
    let (_, entries) = provider::testing::timeline_entries(&timeline, "agy-agent").unwrap();
    assert!(entries.iter().any(|row| row.item["type"] == "tool_call"));
    let text: String = entries
        .iter()
        .filter(|row| row.item["type"] == "assistant_message")
        .filter_map(|row| row.item["text"].as_str())
        .collect();
    assert_eq!(text, "tool");
    manager.close_all().await.unwrap();
    let mut restored =
        Manager::new(AgentManager::new(Box::new(registry)).with_timeline(timeline.clone()));
    restored.register(Box::new(client)).unwrap();
    let resumed = restored.restore("agy-agent").await.unwrap();
    assert_eq!(
        resumed.persistence.as_ref().map(|h| &h.session_id),
        initial.persistence.as_ref().map(|h| &h.session_id)
    );
    let retained = provider::testing::timeline_entries(&timeline, "agy-agent")
        .unwrap()
        .1;
    assert_eq!(retained, entries);
    restored.send("agy-agent", "after restart").await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while restored.active_turn("agy-agent").is_some() {
            restored.poll().await.unwrap();
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let text: String = provider::testing::timeline_entries(&timeline, "agy-agent")
        .unwrap()
        .1
        .iter()
        .filter(|row| row.item["type"] == "assistant_message")
        .filter_map(|row| row.item["text"].as_str())
        .collect();
    assert_eq!(text, "toolafter restart");
    restored.close_all().await.unwrap();
}

#[test]
fn native_evidence_declares_the_verified_agy_log_directory() {
    let (_directory, client, _spec) = fixture();
    let source = client.harness_evidence().unwrap();
    assert_eq!(
        source.log_directory_variable,
        "AIT_DIAGNOSTICS_ANTIGRAVITY_LOG_DIR"
    );
    assert_eq!(
        source.native_logs,
        provider_core::ports::harness_evidence::NativeLogs::UnderRoot {
            root_variable: "HOME",
            home_directory: "",
            subdirectory: ".gemini/antigravity-cli/log",
        }
    );
}
