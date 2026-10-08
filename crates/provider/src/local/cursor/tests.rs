use std::collections::BTreeMap;

use domain::agent_runtime::StoredAgentConfig;
use serde_json::json;
use tempfile::TempDir;

use super::*;
use crate::ports::agent_session::AgentTurnEvent;
use crate::protocol::prompt::{AgentPrompt, PromptImage};

fn fixture(scenario: &str) -> (TempDir, CursorClient, AgentSessionSpec) {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("scenario"), scenario).unwrap();
    let client = CursorClient::new(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/local/cursor/tests/fixtures/acp.py"),
    );
    let spec = AgentSessionSpec {
        provider: PROVIDER.into(),
        cwd: directory.path().to_str().unwrap().into(),
        config: StoredAgentConfig::default(),
    };
    (directory, client, spec)
}

fn requests(directory: &TempDir) -> Vec<Value> {
    std::fs::read_to_string(directory.path().join("requests.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

async fn event(session: &mut dyn AgentSession) -> AgentTurnEvent {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Some(event) = session.poll_turn().unwrap() {
                return event;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn discovers_models_modes_and_authenticates_both_configuration_formats() {
    for scenario in ["", "legacy", "hybrid"] {
        let (directory, client, mut spec) = fixture(scenario);
        let details = client.discover(&spec.cwd).await.unwrap();
        assert_eq!(details.models.len(), 2);
        assert_eq!(details.models[0]["provider"], "cursor");
        assert_eq!(details.modes.len(), 3);
        assert_eq!(details.modes[1]["colorTier"], "planning");
        assert!(!client.supports_history_replay());
        assert!(!client.supports_session_import());
        assert_eq!(
            client.settings(&spec.config)["capabilities"]["supportsDynamicModes"],
            true
        );
        spec.config.model = Some("composer".into());
        spec.config.mode_id = Some("plan".into());
        client.validate_selection(&spec).await.unwrap();
        assert!(requests(&directory).iter().all(|request| {
            !request["method"]
                .as_str()
                .unwrap_or("")
                .starts_with("session/set_")
        }));
        let mut session = client.create_session(&spec).await.unwrap();
        let runtime = session.runtime_info().await.unwrap();
        assert_eq!(runtime.model.as_deref(), Some("composer"));
        assert_eq!(runtime.mode_id.as_deref(), Some("plan"));
        assert_eq!(session.provider(), PROVIDER);
        session.close().await.unwrap();
        session.close().await.unwrap();
        spec.config.model = Some("unknown".into());
        assert!(client.validate_selection(&spec).await.is_err());
        assert!(
            requests(&directory)
                .iter()
                .any(|request| request["method"] == "authenticate")
        );
    }
}

#[tokio::test]
async fn streams_and_answers_native_approvals_questions_and_plans() {
    let (directory, client, spec) = fixture("");
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("hello", &spec.config).await.unwrap();
    assert_eq!(
        session.start_turn("busy", &spec.config).await,
        Err(AgentSessionError::Rejected)
    );
    let mut timeline = Vec::new();
    let mut approvals = 0;
    let mut usage = false;
    loop {
        match event(session.as_mut()).await {
            AgentTurnEvent::PermissionRequested(request) => {
                approvals += 1;
                assert_eq!(request["provider"], "cursor");
                assert_eq!(session.pending_permissions(), vec![request.clone()]);
                let response = if request["kind"] == "question" {
                    json!({"behavior":"allow","updatedInput":{"answers":{"language":["Rust"]}}})
                } else {
                    json!({"behavior":"allow"})
                };
                let id = request["id"].as_str().unwrap();
                assert!(
                    session
                        .respond_permission(id, &json!({"behavior":"invalid"}))
                        .await
                        .is_err()
                );
                session.respond_permission(id, &response).await.unwrap();
                assert!(session.pending_permissions().is_empty());
                assert!(session.respond_permission(id, &response).await.is_err());
            }
            AgentTurnEvent::Timeline(entry) => timeline.push(entry),
            AgentTurnEvent::Usage(snapshot) => {
                assert_eq!(snapshot.context_window_used_tokens, Some(123));
                usage = true;
            }
            AgentTurnEvent::Completed(text) => {
                assert_eq!(text.as_deref(), Some("Hello world"));
                break;
            }
            AgentTurnEvent::RuntimeInfo(_)
            | AgentTurnEvent::Progress { .. }
            | AgentTurnEvent::PermissionResolved(_) => {}
            unexpected => panic!("unexpected: {unexpected:?}"),
        }
    }
    assert_eq!(approvals, 3);
    assert!(usage);
    assert_eq!(timeline.len(), 3);
    assert_eq!(timeline[1].item["status"], "completed");
    assert_eq!(timeline[1].item["detail"]["input"]["command"], "pwd");
    session.close().await.unwrap();
    assert!(
        requests(&directory)
            .iter()
            .any(|message| message["id"] == 44 && message["result"].is_object())
    );
}

#[tokio::test]
async fn resumes_cancels_and_keeps_environment_out_of_persistence() {
    let (directory, client, spec) = fixture("");
    let environment = AgentEnvironment::try_from(BTreeMap::from([(
        "CURSOR_TEST_ENV".into(),
        "ephemeral".into(),
    )]))
    .unwrap();
    let mut session = client
        .create_session_with_environment(&spec, &environment)
        .await
        .unwrap();
    let handle = session.persistence().unwrap();
    assert!(
        !serde_json::to_string(&handle)
            .unwrap()
            .contains("ephemeral")
    );
    session.close().await.unwrap();
    assert!(
        requests(&directory)
            .iter()
            .all(|message| message["hasEnvironment"] == true)
    );
    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    let turn = resumed.start_turn("wait", &spec.config).await.unwrap();
    assert!(resumed.cancel_turn("foreign").await.is_err());
    resumed.cancel_turn(&turn).await.unwrap();
    while event(resumed.as_mut()).await != AgentTurnEvent::Cancelled {}
    resumed.close().await.unwrap();
    assert!(
        requests(&directory)
            .iter()
            .any(|message| message["method"] == "session/load")
    );
    assert!(
        client
            .resume_session(&handle, &spec, AgentResumePurpose::History)
            .await
            .is_err()
    );
    for (provider, id) in [("codex", "native-one"), ("cursor", ""), ("cursor", "bad\n")] {
        let mut invalid = handle.clone();
        invalid.provider = provider.into();
        invalid.session_id = id.into();
        assert!(
            client
                .resume_session(&invalid, &spec, AgentResumePurpose::Interactive)
                .await
                .is_err()
        );
    }
    let mut wrong = handle.clone();
    wrong.metadata = None;
    assert!(
        client
            .resume_session(&wrong, &spec, AgentResumePurpose::Interactive)
            .await
            .is_err()
    );
    std::fs::write(directory.path().join("scenario"), "no-load").unwrap();
    assert!(
        client
            .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn rejects_invalid_frames_authentication_and_deadlines() {
    for scenario in [
        "bad-version",
        "malformed",
        "oversized",
        "hung",
        "unauthorized",
        "missing-catalog",
        "catalog-error",
        "malformed-catalog",
    ] {
        let (_directory, mut client, spec) = fixture(scenario);
        client.deadline = Duration::from_millis(300);
        assert!(
            tokio::time::timeout(Duration::from_secs(3), client.create_session(&spec))
                .await
                .unwrap()
                .is_err(),
            "{scenario}"
        );
    }
    let (_directory, mut client, spec) = fixture("");
    client.program = PathBuf::from("/missing/cursor-agent");
    assert!(!client.is_available().await.unwrap());
    assert!(client.diagnostic().await.unwrap().contains("unavailable"));
    assert!(matches!(
        client.create_session(&spec).await,
        Err(AgentSessionError::Unavailable)
    ));
}

#[tokio::test]
async fn image_input_requires_capability_and_foreign_updates_fail() {
    let prompt = AgentPrompt {
        text: "wait".into(),
        images: vec![PromptImage {
            data: "aGVsbG8=".into(),
            mime_type: "image/png".into(),
        }],
        ..AgentPrompt::default()
    };
    for (scenario, accepted) in [("", false), ("images", true)] {
        let (_directory, client, spec) = fixture(scenario);
        let mut session = client.create_session(&spec).await.unwrap();
        let turn = session.start_input(&prompt, &spec.config).await;
        assert_eq!(turn.is_ok(), accepted);
        if let Ok(turn) = turn {
            session.cancel_turn(&turn).await.unwrap();
            while event(session.as_mut()).await != AgentTurnEvent::Cancelled {}
        }
        session.close().await.unwrap();
    }
    let (_directory, client, spec) = fixture("foreign");
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("hello", &spec.config).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if session.poll_turn().is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    session.close().await.unwrap();
}

#[tokio::test]
async fn catalog_exposes_cursor_label_and_native_default_mode() {
    let (directory, client, spec) = fixture("");
    let mut manager = crate::service::agent_manager::AgentManager::new(Box::new(
        file::storage::agent_runtime::FileBackedAgentRuntimeRegistry::new(
            directory.path().join("agents.json"),
        ),
    ));
    manager.register_client(Box::new(client)).unwrap();
    let snapshot = manager
        .providers("provider.snapshot.get.request", json!({"cwd":spec.cwd}))
        .await
        .unwrap();
    assert_eq!(snapshot["entries"][0]["label"], "Cursor");
    assert_eq!(snapshot["entries"][0]["status"], "ready");
    assert_eq!(snapshot["entries"][0]["defaultModeId"], "agent");
}

#[tokio::test]
async fn hybrid_configuration_uses_per_model_features_and_keeps_selected_identity() {
    let (directory, client, mut spec) = fixture("hybrid");
    spec.config.model = Some("composer".into());
    spec.config.mode_id = Some("plan".into());
    spec.config.thinking_option_id = Some("high".into());
    spec.config.feature_values = Some(BTreeMap::from([("fast_mode".into(), json!(true))]));
    client.validate_selection(&spec).await.unwrap();
    let features = client.draft_features(&spec).await.unwrap();
    assert_eq!(features[0]["id"], "fast_mode");
    assert_eq!(features[0]["value"], true);
    let commands = client.commands(&spec).await.unwrap();
    assert_eq!(commands[0]["name"], "compact");
    assert_eq!(commands[0]["argumentHint"], "[instructions]");
    assert!(requests(&directory).iter().all(|request| {
        !request["method"]
            .as_str()
            .unwrap_or("")
            .starts_with("session/set_")
    }));
    let mut session = client.create_session(&spec).await.unwrap();
    let runtime = session.runtime_info().await.unwrap();
    assert_eq!(runtime.model.as_deref(), Some("composer"));
    assert_eq!(runtime.mode_id.as_deref(), Some("plan"));
    assert_eq!(runtime.thinking_option_id.as_deref(), Some("high"));
    assert_eq!(
        session.control_settings(&spec.config).unwrap()["features"][0]["value"],
        true
    );
    assert_eq!(
        session
            .control_settings(&StoredAgentConfig::default())
            .unwrap()["features"][0]["value"],
        true
    );
    session.start_turn("hello", &spec.config).await.unwrap();
    let mut todo = false;
    let mut usage = false;
    loop {
        match event(session.as_mut()).await {
            AgentTurnEvent::PermissionRequested(request) => {
                let response = if request["kind"] == "question" {
                    json!({"behavior":"allow","updatedInput":{"answers":{"language":["Rust"]}}})
                } else {
                    json!({"behavior":"allow"})
                };
                session
                    .respond_permission(request["id"].as_str().unwrap(), &response)
                    .await
                    .unwrap();
            }
            AgentTurnEvent::Timeline(entry) if entry.item["type"] == "todo" => {
                assert_eq!(
                    entry.item["items"][0],
                    json!({"text":"Implement","completed":false})
                );
                todo = true;
            }
            AgentTurnEvent::Usage(snapshot) if snapshot.input_tokens.is_some() => {
                assert_eq!(snapshot.input_tokens, Some(42));
                assert_eq!(snapshot.output_tokens, Some(7));
                assert_eq!(snapshot.cached_input_tokens, Some(12));
                assert_eq!(snapshot.context_window_used_tokens, Some(123));
                usage = true;
            }
            AgentTurnEvent::Completed(_) => break,
            AgentTurnEvent::Failed | AgentTurnEvent::Cancelled => panic!("turn failed"),
            _ => {}
        }
    }
    assert!(todo && usage);
    let runtime = session.runtime_info().await.unwrap();
    assert_eq!(runtime.model.as_deref(), Some("composer"));
    assert_eq!(runtime.mode_id.as_deref(), Some("plan"));
    assert_eq!(runtime.thinking_option_id.as_deref(), Some("high"));
    let mut unsupported = spec.clone();
    unsupported.config.model = Some("auto".into());
    unsupported.config.thinking_option_id = None;
    assert_eq!(
        session.validate_config_update(&unsupported.config),
        Err(AgentSessionError::Rejected)
    );
    assert!(client.validate_selection(&unsupported).await.is_err());
    assert!(
        client
            .draft_features(&unsupported)
            .await
            .unwrap()
            .is_empty()
    );
    unsupported.config.feature_values = Some(BTreeMap::from([("fast_mode".into(), json!(false))]));
    client.validate_selection(&unsupported).await.unwrap();
    session.close().await.unwrap();
}

#[tokio::test]
async fn resume_validates_native_identity_and_does_not_replay_history_into_new_turn() {
    for scenario in ["replay", "resume-only", "wrong-resume"] {
        let (directory, client, spec) = fixture(scenario);
        let mut session = client.create_session(&spec).await.unwrap();
        let handle = session.persistence().unwrap();
        session.close().await.unwrap();
        let resumed = client
            .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
            .await;
        if scenario == "wrong-resume" {
            assert!(resumed.is_err());
            continue;
        }
        let mut resumed = resumed.unwrap();
        let turn = resumed.start_turn("wait", &spec.config).await.unwrap();
        resumed.cancel_turn(&turn).await.unwrap();
        loop {
            match event(resumed.as_mut()).await {
                AgentTurnEvent::Cancelled => break,
                AgentTurnEvent::RuntimeInfo(_) => {}
                unexpected => panic!("history leaked: {unexpected:?}"),
            }
        }
        resumed.close().await.unwrap();
        let method = if scenario == "resume-only" {
            "session/resume"
        } else {
            "session/load"
        };
        assert!(
            requests(&directory)
                .iter()
                .any(|request| request["method"] == method)
        );
    }
}

#[tokio::test]
async fn cancellation_finalizes_unfinished_tools_and_resolves_pending_callbacks() {
    for scenario in ["cancel-tools", ""] {
        let (directory, client, spec) = fixture(scenario);
        let mut session = client.create_session(&spec).await.unwrap();
        let turn = session
            .start_turn(
                if scenario == "cancel-tools" {
                    "wait"
                } else {
                    "hello"
                },
                &spec.config,
            )
            .await
            .unwrap();
        loop {
            let event = event(session.as_mut()).await;
            if (scenario == "cancel-tools" && matches!(event, AgentTurnEvent::Progress { .. }))
                || (scenario.is_empty() && matches!(event, AgentTurnEvent::PermissionRequested(_)))
            {
                break;
            }
        }
        session.cancel_turn(&turn).await.unwrap();
        let mut finalized = false;
        loop {
            match event(session.as_mut()).await {
                AgentTurnEvent::Timeline(entry) if entry.item["type"] == "tool_call" => {
                    assert_eq!(entry.item["status"], "failed");
                    assert_eq!(entry.item["error"]["message"], "Turn cancelled");
                    assert!(entry.item["detail"]["input"]["command"].is_string());
                    finalized = true;
                }
                AgentTurnEvent::Cancelled => break,
                AgentTurnEvent::Failed | AgentTurnEvent::Completed(_) => {
                    panic!("unexpected completion")
                }
                _ => {}
            }
        }
        assert!(finalized);
        assert!(session.pending_permissions().is_empty());
        session.close().await.unwrap();
        if scenario.is_empty() {
            assert!(
                requests(&directory)
                    .iter()
                    .any(|reply| reply["id"] == "permission"
                        && reply["result"]["outcome"]["outcome"] == "cancelled")
            );
        }
    }
}

#[tokio::test]
async fn empty_native_model_catalog_does_not_fall_back_to_session_models() {
    let (_directory, client, spec) = fixture("empty-catalog");
    assert!(client.discover(&spec.cwd).await.unwrap().models.is_empty());
}

#[tokio::test]
async fn command_discovery_wait_is_bounded_and_closes_the_probe() {
    let (directory, client, spec) = fixture("no-commands");
    let commands = tokio::time::timeout(Duration::from_secs(12), client.commands(&spec))
        .await
        .unwrap()
        .unwrap();
    assert!(commands.is_empty());
    assert!(
        requests(&directory)
            .iter()
            .all(|request| request["method"] != "session/prompt")
    );
}
