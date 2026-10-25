use std::collections::BTreeMap;
use std::time::Duration;

use super::*;
use crate::ports::agent_session::AgentTurnEvent;
use crate::protocol::prompt::{AgentPrompt, PromptImage};

mod capabilities;
mod diagnostics;
mod installed;
mod real;

#[tokio::test]
async fn a_previous_settlement_never_finishes_the_next_prompt_or_its_background_work() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    for (text, expected) in [
        ("first", "Hello world"),
        ("second", "Hello world"),
        ("background", "Background finished"),
        ("last", "Hello world"),
    ] {
        session.start_turn(text, &spec.config).await.unwrap();
        let events = complete(session.as_mut()).await;
        assert!(
            matches!(events.last(), Some(AgentTurnEvent::Completed(Some(reply))) if reply == expected)
        );
        assert!(events.iter().any(|event|matches!(event,AgentTurnEvent::Timeline(entry) if entry.item["type"] == "user_message" && entry.item["text"] == text)));
    }
    session.close().await.unwrap();
}

#[tokio::test]
async fn manager_persists_and_restores_the_same_timeline_generation() {
    use crate::service::agent_manager::{AgentManager, AgentRegistration};
    use crate::storage::timeline::Timeline;
    use domain::agent_runtime::registry::AgentRuntimeRegistry;
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
    let (root, client, spec) = fixture();
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    let timeline = Timeline::open(&root.path().join("timeline.sqlite")).unwrap();
    let mut manager = AgentManager::new(Box::new(registry.clone())).with_timeline(timeline.clone());
    manager.register_client(Box::new(client.clone())).unwrap();
    manager
        .create("agent", &spec, AgentRegistration::default())
        .await
        .unwrap();
    manager.send("agent", "subagents").await.unwrap();
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
        domain::agent_runtime::AgentRuntimeStatus::Idle
    );
    let (epoch, rows) = timeline.read("agent").unwrap();
    assert!(
        rows.iter()
            .any(|row| row.entry.item["text"] == "Hello world")
    );
    assert_eq!(timeline.subagents("agent").unwrap().len(), 1);
    manager.close_all().await.unwrap();
    let mut manager = AgentManager::new(Box::new(registry)).with_timeline(timeline.clone());
    manager.register_client(Box::new(client)).unwrap();
    manager.resume("agent").await.unwrap();
    assert_eq!(timeline.read("agent").unwrap().0, epoch);
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn empty_agents_reopen_without_manufacturing_or_resending_input() {
    let (_root, client, mut spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    assert_eq!(client.history(&handle, &spec.cwd).await.unwrap().len(), 0);
    let mut session = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    spec.config.mode_id = Some("write".to_owned());
    session.start_turn("hello", &spec.config).await.unwrap();
    complete(session.as_mut()).await;
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let path = handle.metadata.as_ref().unwrap()["sessionFile"]
        .as_str()
        .unwrap();
    std::fs::remove_file(path).unwrap();
    assert!(client.history(&handle, &spec.cwd).await.is_err());
}

#[tokio::test]
async fn native_modes_questions_children_and_chunked_frames() {
    let (root, client, mut spec) = fixture();
    spec.config.mode_id = Some("ask".to_owned());
    let mut session = client.create_session(&spec).await.unwrap();
    assert_eq!(
        session.runtime_info().await.unwrap().mode_id.as_deref(),
        Some("ask")
    );
    session.start_turn("ask", &spec.config).await.unwrap();
    let request = loop {
        if let AgentTurnEvent::PermissionRequested(request) = next(session.as_mut()).await.unwrap()
        {
            break request;
        }
    };
    assert_eq!(request["input"]["questions"][1]["answerFormat"], "array");
    session
        .respond_permission(
            request["id"].as_str().unwrap(),
            &json!({"behavior":"allow",
        "updatedInput":{"answers":{"db":["A,B"],"features":["Auth","Search"]}}}),
        )
        .await
        .unwrap();
    complete(session.as_mut()).await;
    session.start_turn("subagents", &spec.config).await.unwrap();
    let events = complete(session.as_mut()).await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentTurnEvent::Subagent(_)))
    );
    assert_eq!(session.subagents().len(), 1);
    assert_eq!(session.subagents()[0].descriptor["status"], "completed");
    session.start_turn("chunked", &spec.config).await.unwrap();
    complete(session.as_mut()).await;
    spec.config.mode_id = Some("write".to_owned());
    session.start_turn("changed", &spec.config).await.unwrap();
    complete(session.as_mut()).await;
    session.close().await.unwrap();
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(calls.contains("always-ask"));
    assert!(calls.contains("\"write\""));
    assert!(calls.contains("negotiate_protocol"));
}

fn fixture() -> (tempfile::TempDir, OmpClient, AgentSessionSpec) {
    let root = tempfile::tempdir().unwrap();
    let program = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/local/omp/tests/fixtures/omp.py");
    let client = OmpClient {
        environment: AgentEnvironment::try_from(BTreeMap::from([(
            "PI_CODING_AGENT_DIR".to_owned(),
            root.path().join("agent").to_str().unwrap().to_owned(),
        )]))
        .unwrap(),
        ..OmpClient::new(program).with_image_directory(root.path().join("images"))
    };
    let spec = AgentSessionSpec {
        provider: "omp".to_owned(),
        cwd: root.path().to_str().unwrap().to_owned(),
        config: StoredAgentConfig::default(),
    };
    (root, client, spec)
}

pub(super) async fn next(
    session: &mut dyn AgentSession,
) -> Result<AgentTurnEvent, AgentSessionError> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(event) = session.poll_turn()? {
                return Ok(event);
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap()
}

pub(super) async fn complete(session: &mut dyn AgentSession) -> Vec<AgentTurnEvent> {
    let mut events = Vec::new();
    loop {
        let event = next(session).await.unwrap();
        let done = matches!(
            event,
            AgentTurnEvent::Completed(_) | AgentTurnEvent::Cancelled | AgentTurnEvent::Failed
        );
        events.push(event);
        if done {
            return events;
        }
    }
}

#[tokio::test]
async fn discovery_commands_and_diagnostics_do_not_create_history_or_prompt() {
    let (root, client, spec) = fixture();
    assert!(client.is_available().await.unwrap());
    assert!(client.supports_session_import());
    let details = client.discover(&spec.cwd).await.unwrap();
    assert_eq!(details.models.len(), 2);
    assert_ne!(details.models[0]["id"], details.models[1]["id"]);
    assert_eq!(client.commands(&spec).await.unwrap()[0]["name"], "review");
    assert!(
        client
            .diagnostic()
            .await
            .unwrap()
            .contains("2 available models")
    );
    assert!(!root.path().join("agent/sessions").exists());
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(!calls.contains("\"type\": \"prompt\""));
}

#[tokio::test]
async fn rich_turns_replay_without_duplicate_items_and_resume_the_same_session() {
    let (_root, client, mut spec) = fixture();
    spec.config.model = Some(json!(["test", "model"]).to_string());
    spec.config.thinking_option_id = Some("high".to_owned());
    let mut session = client.create_session(&spec).await.unwrap();
    assert_eq!(
        session.runtime_info().await.unwrap().model,
        spec.config.model
    );
    let prompt = AgentPrompt {
        text: "hello".to_owned(),
        client_message_id: Some("client-1".to_owned()),
        images: vec![PromptImage {
            data: "aW1hZ2U=".to_owned(),
            mime_type: "image/png".to_owned(),
        }],
        ..AgentPrompt::default()
    };
    session.start_input(&prompt, &spec.config).await.unwrap();
    assert!(session.start_turn("busy", &spec.config).await.is_err());
    let events = complete(session.as_mut()).await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentTurnEvent::Progress { .. }))
    );
    assert!(events.iter().any(|event|matches!(event,AgentTurnEvent::Timeline(entry) if entry.item["clientMessageId"]=="client-1")));
    assert!(events.iter().any(|event|matches!(event,AgentTurnEvent::Timeline(entry) if entry.item["detail"]["type"]=="read")));
    let handle = session.persistence().unwrap();
    let live_keys: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Timeline(entry) => Some(entry.key.clone()),
            _ => None,
        })
        .collect();
    session.close().await.unwrap();
    let history = client.history(&handle, &spec.cwd).await.unwrap();
    assert_eq!(
        live_keys,
        history
            .iter()
            .map(|entry| entry.key.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(history[0].item["clientMessageId"], "client-1");
    let sessions = client
        .list_sessions(&ListOptions {
            cwd: Some(spec.cwd.clone()),
            scan_limit: 100,
        })
        .await
        .unwrap();
    assert_eq!(sessions.len(), 1);
    let imported = AgentPersistenceHandle {
        metadata: None,
        ..handle.clone()
    };
    assert_eq!(
        client
            .inspect_session(&imported, &spec.cwd)
            .await
            .unwrap()
            .descriptor
            .provider_handle_id,
        handle.session_id
    );
    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    resumed.start_turn("again", &spec.config).await.unwrap();
    complete(resumed.as_mut()).await;
    assert_eq!(resumed.persistence().unwrap().session_id, handle.session_id);
    resumed.close().await.unwrap();
    assert!(client.history(&handle, "/wrong-directory").await.is_err());
}

#[tokio::test]
async fn agent_end_does_not_complete_retries_and_steering_is_acknowledged() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    assert!(matches!(
        next(session.as_mut()).await.unwrap(),
        AgentTurnEvent::Timeline(_)
    ));
    for _ in 0..10 {
        assert!(!matches!(
            session.poll_turn().unwrap(),
            Some(AgentTurnEvent::Completed(_))
        ));
    }
    session.steer_turn(&turn, "continue").await.unwrap();
    assert!(matches!(
        complete(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    session.cancel_turn(&turn).await.unwrap();
    assert!(matches!(
        complete(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Cancelled)
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn questions_require_exact_answers_and_unsupported_input_never_reaches_native() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let invalid = AgentPrompt {
        output_schema: Some(json!({"type":"object"})),
        ..AgentPrompt::text("invalid")
    };
    assert_eq!(
        session.start_input(&invalid, &spec.config).await,
        Err(AgentSessionError::Rejected)
    );
    assert_eq!(
        session.start_turn("reject", &spec.config).await,
        Err(AgentSessionError::Rejected)
    );
    session.start_turn("question", &spec.config).await.unwrap();
    let request = loop {
        if let AgentTurnEvent::PermissionRequested(request) = next(session.as_mut()).await.unwrap()
        {
            break request;
        }
    };
    assert_eq!(session.pending_permissions().len(), 1);
    let id = request["id"].as_str().unwrap();
    assert!(
        session
            .respond_permission(
                id,
                &json!({"behavior":"allow","updatedInput":{"answers":{"value":"invalid"}}})
            )
            .await
            .is_err()
    );
    session
        .respond_permission(
            id,
            &json!({"behavior":"allow","updatedInput":{"answers":{"value":"A"}}}),
        )
        .await
        .unwrap();
    complete(session.as_mut()).await;
    assert!(
        session
            .respond_permission(id, &json!({"behavior":"deny"}))
            .await
            .is_err()
    );
    session.start_turn("/local", &spec.config).await.unwrap();
    assert!(matches!(
        complete(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(None))
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn malformed_output_fails_without_resending_input() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("malformed", &spec.config).await.unwrap();
    loop {
        if next(session.as_mut()).await.is_err() {
            break;
        }
    }
    session.close().await.unwrap();
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert_eq!(
        calls
            .lines()
            .filter(|line| line.contains("\"type\": \"prompt\""))
            .count(),
        1
    );
}
