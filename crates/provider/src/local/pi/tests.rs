use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;

mod admission;
mod diagnostics;
mod installed;
mod preview;
mod review;
mod text_storage;
mod tool_schema;
use crate::ports::agent_session::AgentTurnEvent;
use crate::ports::diagnostics::Category;
use crate::protocol::prompt::AgentPrompt;

fn fixture(scenario: &str) -> (TempDir, PiClient, AgentSessionSpec) {
    let directory = tempfile::tempdir().unwrap();
    let cwd = directory.path().canonicalize().unwrap();
    let program = cwd.join("pi");
    std::fs::write(&program, include_str!("tests/fixtures/pi.py")).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut client = PiClient::new(program);
    client.session_environment = history::SessionEnvironment::isolated(&cwd, &cwd);
    client.environment = AgentEnvironment::try_from(BTreeMap::from([(
        "PI_FIXTURE_SCENARIO".into(),
        scenario.into(),
    )]))
    .unwrap();
    let spec = AgentSessionSpec {
        provider: PROVIDER.into(),
        cwd: cwd.to_str().unwrap().into(),
        config: StoredAgentConfig::default(),
    };
    (directory, client, spec)
}

async fn terminal(session: &mut dyn AgentSession) -> Vec<AgentTurnEvent> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut events = Vec::new();
        loop {
            if let Some(event) = session.poll_turn().unwrap() {
                let done = matches!(
                    event,
                    AgentTurnEvent::Completed(_)
                        | AgentTurnEvent::Cancelled
                        | AgentTurnEvent::Failed
                );
                events.push(event);
                if done {
                    return events;
                }
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn discovers_without_persistence_and_rejects_old_protocol() {
    let (directory, client, spec) = fixture("");
    assert!(client.is_available().await.unwrap());
    let details = client.discover(&spec.cwd).await.unwrap();
    assert_eq!(details.models[0]["id"], "fixture/model");
    assert!(details.modes.is_empty());
    assert_eq!(client.settings(&spec.config)["availableModes"], json!([]));
    assert!(!directory.path().join("session.jsonl").exists());
    assert!(!client.supports_history_replay());
    assert!(client.supports_session_import());
    assert!(client.diagnostic().await.unwrap().contains("Pi RPC"));
    let (_directory, client, spec) = fixture("old");
    assert!(matches!(
        client.discover(&spec.cwd).await,
        Err(AgentSessionError::Unavailable)
    ));
}

#[tokio::test]
async fn streams_tools_usage_and_resumes_same_native_identity() {
    let (directory, client, mut spec) = fixture("");
    spec.config.model = Some("fixture/model".into());
    client.validate_selection(&spec).await.unwrap();
    let mut session = client.create_session(&spec).await.unwrap();
    let arguments: Vec<String> = serde_json::from_str(
        &std::fs::read_to_string(directory.path().join("arguments.json")).unwrap(),
    )
    .unwrap();
    let session_directory =
        history::session_directory(&client, std::path::Path::new(&spec.cwd)).unwrap();
    assert_eq!(
        arguments
            .windows(2)
            .find(|arguments| arguments[0] == "--session-dir")
            .map(|arguments| arguments[1].as_str()),
        session_directory.to_str()
    );
    let initial = session.persistence().unwrap();
    let runtime_info = session.runtime_info().await.unwrap();
    assert_eq!(runtime_info.model.as_deref(), Some("fixture/model"));
    assert_eq!(runtime_info.mode_id, None);
    session.start_turn("hello", &spec.config).await.unwrap();
    let events = terminal(session.as_mut()).await;
    assert!(
        matches!(events.last(), Some(AgentTurnEvent::Completed(Some(text))) if text == "Hello")
    );
    assert!(events.iter().any(
        |event| matches!(event, AgentTurnEvent::Usage(usage) if usage.input_tokens == Some(12))
    ));
    assert!(events.iter().any(|event| matches!(event, AgentTurnEvent::Progress{ entry,.. } if entry.item["text"] == "Hello")));
    assert!(events.iter().any(|event| matches!(event, AgentTurnEvent::Timeline(entry) if entry.item["type"] == "tool_call" && entry.item["status"] == "completed")));
    let handle = session.persistence().unwrap();
    assert_eq!(initial.session_id, handle.session_id);
    session.close().await.unwrap();
    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert_eq!(resumed.persistence().unwrap().session_id, handle.session_id);
    resumed.start_turn("again", &spec.config).await.unwrap();
    assert!(matches!(
        terminal(resumed.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    resumed.close().await.unwrap();
    assert!(
        client
            .resume_session(&handle, &spec, AgentResumePurpose::History)
            .await
            .is_err()
    );
    let mut invalid = handle;
    invalid.session_id = "invalid".to_owned();
    assert!(
        client
            .resume_session(&invalid, &spec, AgentResumePurpose::Interactive)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn creates_and_resumes_without_a_host_home() {
    let (directory, mut client, spec) = fixture("");
    client.session_environment = history::SessionEnvironment::without_home(directory.path());

    let mut session = client.create_session(&spec).await.unwrap();
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    resumed.close().await.unwrap();

    let arguments: Vec<String> = serde_json::from_str(
        &std::fs::read_to_string(directory.path().join("arguments.json")).unwrap(),
    )
    .unwrap();
    assert!(!arguments.iter().any(|argument| argument == "--session-dir"));
}

#[tokio::test]
async fn resume_rejects_foreign_provider_and_mismatched_cwd() {
    let (_directory, client, spec) = fixture("");
    let mut session = client.create_session(&spec).await.unwrap();
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();

    let mut foreign = handle.clone();
    foreign.provider = "claude".into();
    assert!(matches!(
        client
            .resume_session(&foreign, &spec, AgentResumePurpose::Interactive)
            .await,
        Err(AgentSessionError::Rejected)
    ));

    let mut mismatched_cwd = handle;
    mismatched_cwd
        .metadata
        .as_mut()
        .unwrap()
        .insert("cwd".into(), json!("/"));
    assert!(matches!(
        client
            .resume_session(&mismatched_cwd, &spec, AgentResumePurpose::Interactive)
            .await,
        Err(AgentSessionError::Rejected)
    ));
}

#[tokio::test]
async fn model_less_create_and_resume_each_fetch_state_once() {
    let (directory, client, spec) = fixture("");
    let mut session = client.create_session(&spec).await.unwrap();
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();

    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    resumed.close().await.unwrap();

    let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    let request_types: Vec<_> = requests
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap()["type"].clone())
        .collect();
    assert_eq!(request_types, ["get_state", "get_state"]);
}

#[tokio::test]
async fn session_identity_guard_rejects_swaps_at_every_state_boundary() {
    let (_directory, client, spec) = fixture("session-swap-open");
    assert!(matches!(
        client.create_session(&spec).await,
        Err(AgentSessionError::Failed)
    ));

    let (_directory, client, spec) = fixture("session-swap-configure");
    let mut session = client.create_session(&spec).await.unwrap();
    let mut changed_config = spec.config.clone();
    changed_config.model = Some("fixture/model".into());
    assert!(matches!(
        session.start_turn("hello", &changed_config).await,
        Err(AgentSessionError::Failed)
    ));
    assert!(matches!(
        session.start_turn("retry", &changed_config).await,
        Err(AgentSessionError::Rejected)
    ));

    let (_directory, client, spec) = fixture("session-swap-synchronize");
    let mut session = client.create_session(&spec).await.unwrap();
    assert!(matches!(
        session.start_turn("hello", &spec.config).await,
        Err(AgentSessionError::Failed)
    ));
    let diagnostic = session.failure_diagnostic().unwrap();
    assert_eq!(diagnostic.category, Category::Protocol);
    assert_eq!(diagnostic.source, "session_identity");
    assert!(matches!(
        session.start_turn("retry", &spec.config).await,
        Err(AgentSessionError::Rejected)
    ));
}

#[tokio::test]
async fn cancellation_waits_for_settlement_and_allows_next_turn() {
    let (_directory, client, spec) = fixture("");
    let mut session = client.create_session(&spec).await.unwrap();
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    assert!(session.start_turn("second", &spec.config).await.is_err());
    assert!(session.cancel_turn("wrong").await.is_err());
    session.cancel_turn(&turn).await.unwrap();
    assert!(matches!(
        terminal(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Cancelled)
    ));
    session.start_turn("hello", &spec.config).await.unwrap();
    assert!(matches!(
        terminal(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn rejection_errors_handled_commands_and_extension_dialogs_do_not_hang() {
    for text in ["reject", "exit", "malformed", "handled", "dialog", "fail"] {
        let (_directory, client, spec) = fixture("");
        let mut session = client.create_session(&spec).await.unwrap();
        let start = session.start_turn(text, &spec.config).await;
        if matches!(text, "reject" | "exit" | "malformed") {
            assert!(start.is_err(), "{text}");
        } else {
            start.unwrap();
            let events = terminal(session.as_mut()).await;
            if text == "fail" {
                assert!(matches!(events.last(), Some(AgentTurnEvent::Failed)));
            } else {
                assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
            }
        }
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn unsupported_configuration_and_structured_output_are_rejected_before_submission() {
    let (directory, client, spec) = fixture("");
    let mut config = spec.config.clone();
    config.mode_id = Some("default".into());
    client.validate_config(&config).unwrap();
    config = spec.config.clone();
    config.mode_id = Some("plan".into());
    assert!(client.validate_config(&config).is_err());
    config = spec.config.clone();
    config.model = Some("unqualified".into());
    assert!(client.validate_config(&config).is_err());
    config = spec.config.clone();
    config.thinking_option_id = Some("high".into());
    assert!(client.validate_config(&config).is_err());
    let mut session = client
        .create_session_with_environment(&spec, &client.environment)
        .await
        .unwrap();
    let prompt = AgentPrompt {
        output_schema: Some(json!({"type":"object"})),
        ..AgentPrompt::text("hello")
    };
    assert!(session.start_input(&prompt, &spec.config).await.is_err());
    session.close().await.unwrap();
    let log = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    assert!(
        !log.lines()
            .any(|line| serde_json::from_str::<Value>(line).unwrap()["type"] == "prompt")
    );
}

#[test]
fn stream_does_not_finish_at_agent_end_and_bounds_display_output() {
    let mut stream = streaming::Stream::default();
    stream.begin("turn".into());
    assert!(!stream.consume(&json!({"type":"agent_end"})).unwrap());
    assert!(stream.events.is_empty());
    assert!(
        stream
            .consume(&json!({"type":"message_update","assistantMessageEvent":{
        "type":"text_delta","contentIndex":0,"delta":"x".repeat(128*1024+1)}}))
            .is_err()
    );
    assert!(
        config::models(&json!({"models":[{"provider":"a","id":"b"},{"provider":"a","id":"b"}]}))
            .is_err()
    );
}

#[tokio::test]
async fn failed_native_writer_is_not_reused_and_missing_history_is_not_recreated() {
    let (directory, client, spec) = fixture("");
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("die-after-admission", &spec.config)
        .await
        .unwrap();
    assert!(matches!(
        terminal(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Failed)
    ));
    assert!(session.start_turn("retry", &spec.config).await.is_err());
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    std::fs::remove_file(directory.path().join("session.jsonl")).unwrap();
    assert!(
        client
            .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
            .await
            .is_err()
    );
    assert!(!directory.path().join("session.jsonl").exists());
}

#[tokio::test]
async fn rich_input_preserves_images_and_unicode_framing_without_persisting_environment() {
    use crate::protocol::prompt::PromptImage;
    let (directory, client, spec) = fixture("");
    let mut session = client.create_session(&spec).await.unwrap();
    let prompt = AgentPrompt {
        images: vec![PromptImage {
            data: "aGVsbG8=".into(),
            mime_type: "image/png".into(),
        }],
        ..AgentPrompt::text("line one\nline two\u{2028}end")
    };
    session.start_input(&prompt, &spec.config).await.unwrap();
    terminal(session.as_mut()).await;
    assert!(
        !serde_json::to_string(&session.persistence())
            .unwrap()
            .contains("PI_FIXTURE_SCENARIO")
    );
    session.close().await.unwrap();
    let log = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    let command = log
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|command| command["type"] == "prompt")
        .unwrap();
    assert_eq!(command["message"], prompt.text);
    assert_eq!(command["images"][0]["data"], "aGVsbG8=");
}

#[tokio::test]
async fn manager_retains_timeline_and_native_session_across_restart() {
    use crate::service::agent_manager::{AgentManager, AgentRegistration};
    use crate::storage::timeline::Timeline;
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

    let (directory, client, spec) = fixture("");
    let registry = FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json"));
    let timeline_path = directory.path().join("timeline.sqlite");
    let timeline = Timeline::open(&timeline_path).unwrap();
    let mut manager = AgentManager::new(Box::new(registry.clone())).with_timeline(timeline.clone());
    manager.register_client(Box::new(client.clone())).unwrap();
    manager
        .create("pi-agent", &spec, AgentRegistration::default())
        .await
        .unwrap();
    for index in 0..3 {
        let prompt = AgentPrompt {
            text: format!("hello {index}"),
            client_message_id: Some(format!("client-{index}")),
            ..AgentPrompt::default()
        };
        manager.send_input("pi-agent", &prompt).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while manager.active_turn("pi-agent").is_some() {
                manager.poll().await.unwrap();
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
    }
    assert_eq!(manager.last_message("pi-agent"), Some("Hello"));
    let (_, original) = timeline.read("pi-agent").unwrap();
    assert!(
        original
            .iter()
            .any(|row| row.entry.item["type"] == "tool_call")
    );
    let users: Vec<_> = original
        .iter()
        .filter(|row| row.entry.item["type"] == "user_message")
        .collect();
    assert_eq!(users.len(), 3);
    assert_eq!(original[0].entry.item["type"], "user_message");
    for (index, user) in users.iter().enumerate() {
        assert_eq!(user.entry.item["text"], format!("hello {index}"));
        assert_eq!(
            user.entry.item["clientMessageId"],
            format!("client-{index}")
        );
        let reply = original
            .iter()
            .find(|row| {
                row.entry.turn_id == user.entry.turn_id
                    && row.entry.item["type"] == "assistant_message"
            })
            .unwrap();
        assert!(user.seq < reply.seq);
    }
    manager.close_all().await.unwrap();
    drop(manager);
    drop(timeline);
    let timeline = Timeline::open(&timeline_path).unwrap();
    let mut restored = AgentManager::new(Box::new(registry)).with_timeline(timeline.clone());
    restored.register_client(Box::new(client)).unwrap();
    restored
        .restore("pi-agent", &crate::protocol::resume::Overrides::default())
        .await
        .unwrap();
    let original: Vec<_> = original.into_iter().map(|row| row.entry).collect();
    let retained: Vec<_> = timeline
        .read("pi-agent")
        .unwrap()
        .1
        .into_iter()
        .map(|row| row.entry)
        .collect();
    assert_eq!(original, retained);
    restored.close_all().await.unwrap();
}

#[tokio::test]
async fn missing_and_mismatched_control_responses_fail_without_prompt_replay() {
    for scenario in ["hang", "wrong-response"] {
        let (directory, mut client, spec) = fixture(scenario);
        client.deadline = Duration::from_millis(500);
        let result = client.create_session(&spec).await;
        assert!(
            matches!(result, Err(AgentSessionError::Failed)),
            "{scenario}"
        );
        let log = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
        assert_eq!(log.lines().count(), 1);
        assert_eq!(
            serde_json::from_str::<Value>(log.lines().next().unwrap()).unwrap()["type"],
            "get_state"
        );
    }
}
