use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;
use crate::ports::agent_session::AgentTurnEvent;

const PRO: &str = "[\"deepseek-official\",\"deepseek-v4-pro\"]";
const FLASH: &str = "[\"deepseek-official\",\"deepseek-v4-flash\"]";

#[cfg(unix)]
fn fixture() -> (TempDir, DeepSeekHarnessClient, AgentSessionSpec) {
    let directory = tempfile::tempdir().unwrap();
    // Keep the executable immutable while concurrent tests spawn processes. Executing a
    // freshly written script can fail with ETXTBSY on Linux if another fork inherited
    // its writable descriptor before it was closed. Logs and cwd remain per-test.
    let program = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/local/deepseek_harness/tests/fixtures/acp.cjs");
    let mut client = DeepSeekHarnessClient::new(program)
        .with_acp_profile()
        .with_image_directory(directory.path().join("images"));
    // Successful Node handshakes must tolerate instrumented builds competing for CPU.
    // Timeout behavior is exercised separately with an explicit 300 ms deadline.
    client.deadline = Duration::from_secs(10);
    client.environment = AgentEnvironment::try_from(BTreeMap::from([(
        "ACP_FIXTURE_LOG".to_owned(),
        directory
            .path()
            .join("requests.jsonl")
            .to_str()
            .unwrap()
            .to_owned(),
    )]))
    .unwrap();
    let spec = AgentSessionSpec {
        provider: PROVIDER.to_owned(),
        cwd: directory.path().to_str().unwrap().to_owned(),
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

#[cfg(unix)]
#[tokio::test]
async fn concurrent_acp_fixtures_keep_sessions_and_logs_isolated() {
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..16 {
        tasks.spawn(async {
            let (directory, client, spec) = fixture();
            let mut session = client.create_session(&spec).await.unwrap();
            session.close().await.unwrap();
            let logged = requests(&directory);
            assert_eq!(
                logged
                    .iter()
                    .filter(|request| request["method"] == "session/new")
                    .count(),
                1
            );
        });
    }
    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }
}

#[cfg(unix)]
#[tokio::test]
async fn missing_acp_program_is_unavailable() {
    let (directory, mut client, spec) = fixture();
    client.program = directory.path().join("missing-dsh");
    assert!(matches!(
        client.create_session(&spec).await,
        Err(AgentSessionError::Unavailable)
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn discovers_opaque_grouped_models_and_actual_reasoning_choices() {
    let (directory, client, spec) = fixture();
    assert!(client.is_available().await.unwrap());
    assert!(!client.supports_history_replay());
    let settings = client.settings(&spec.config);
    assert_eq!(settings["availableModes"], json!([]));
    assert_eq!(settings["features"], json!([]));
    assert_eq!(settings["capabilities"]["supportsMcpServers"], true);
    assert_eq!(settings["capabilities"]["supportsSessionListing"], false);
    let details = client.discover(&spec.cwd).await.unwrap();
    assert_eq!(details.models.len(), 2);
    assert!(
        details
            .models
            .iter()
            .all(|model| model.get("description").is_none())
    );
    assert_eq!(details.models[0]["id"], PRO);
    assert_eq!(details.models[0]["label"], "DeepSeek V4 Pro");
    assert_eq!(details.models[0]["defaultThinkingOptionId"], "high");
    assert_eq!(
        details.models[0]["thinkingOptions"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert!(details.modes.is_empty());
    assert!(details.features.is_empty());
    assert!(
        requests(&directory)
            .iter()
            .any(|request| request["method"] == "session/close")
    );
    assert!(client.diagnostic().await.unwrap().contains("ACP v1 ready"));
}

#[cfg(unix)]
#[tokio::test]
async fn streams_tools_approvals_text_and_usage_then_resumes_and_cancels() {
    let (directory, client, mut spec) = fixture();
    spec.config.model = Some(FLASH.to_owned());
    spec.config.thinking_option_id = Some("off".to_owned());
    let mut session = client.create_session(&spec).await.unwrap();
    let info = session.runtime_info().await.unwrap();
    assert_eq!(info.model.as_deref(), Some(FLASH));
    assert_eq!(info.thinking_option_id.as_deref(), Some("off"));
    let handle = session.persistence().unwrap();
    let prompt = crate::protocol::prompt::AgentPrompt {
        text: "hello".to_owned(),
        client_message_id: Some("client-message".to_owned()),
        ..crate::protocol::prompt::AgentPrompt::default()
    };
    session.start_input(&prompt, &spec.config).await.unwrap();
    assert_eq!(
        session.start_turn("busy", &spec.config).await,
        Err(AgentSessionError::Rejected)
    );
    let mut completed = Vec::new();
    let mut usage = false;
    let mut resolved = false;
    loop {
        match event(session.as_mut()).await {
            AgentTurnEvent::PermissionRequested(request) => {
                assert_eq!(request["provider"], PROVIDER);
                assert_eq!(session.pending_permissions(), vec![request.clone()]);
                let id = request["id"].as_str().unwrap();
                assert_eq!(
                    session
                        .respond_permission(
                            id,
                            &json!({"behavior":"allow","selectedActionId":"deny"})
                        )
                        .await,
                    Err(AgentSessionError::Rejected)
                );
                session
                    .respond_permission(id, &json!({"behavior":"allow","selectedActionId":"once"}))
                    .await
                    .unwrap();
                assert!(session.pending_permissions().is_empty());
                assert_eq!(
                    session
                        .respond_permission(id, &json!({"behavior":"allow"}))
                        .await,
                    Err(AgentSessionError::Rejected)
                );
            }
            AgentTurnEvent::PermissionResolved(_) => resolved = true,
            AgentTurnEvent::Timeline(entry) => completed.push(entry),
            AgentTurnEvent::Usage(snapshot) => {
                assert_eq!(snapshot.context_window_used_tokens, Some(123));
                assert_eq!(snapshot.context_window_max_tokens, Some(1000));
                assert_eq!(snapshot.input_tokens, None);
                usage = true;
            }
            AgentTurnEvent::Completed(text) => {
                assert_eq!(text.as_deref(), Some("Hello world"));
                break;
            }
            AgentTurnEvent::Progress { .. } | AgentTurnEvent::RuntimeInfo(_) => {}
            unexpected => panic!("unexpected event: {unexpected:?}"),
        }
    }
    assert!(usage && resolved);
    assert_eq!(completed.len(), 3);
    assert_eq!(completed[0].item["type"], "reasoning");
    assert_eq!(
        completed[1].item["detail"]["input"],
        json!({"command":"pwd"})
    );
    assert_eq!(completed[1].item["status"], "completed");
    assert_eq!(completed[2].item["text"], "Hello world");
    session.close().await.unwrap();
    session.close().await.unwrap();
    assert_resume_and_cancel(&client, &spec, &handle, &directory).await;
}

#[cfg(unix)]
#[tokio::test]
async fn validates_dynamic_efforts_and_rejects_invalid_resume_and_images() {
    let (_directory, client, mut spec) = fixture();
    spec.config.model = Some(FLASH.to_owned());
    spec.config.thinking_option_id = Some("low".to_owned());
    assert_eq!(
        client.validate_selection(&spec).await,
        Err(AgentSessionError::Rejected)
    );
    spec.config.thinking_option_id = Some("off".to_owned());
    client.validate_selection(&spec).await.unwrap();
    let mut session = client.create_session(&spec).await.unwrap();
    let mut handle = session.persistence().unwrap();
    handle.provider = "codex".to_owned();
    assert!(
        client
            .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
            .await
            .is_err()
    );
    handle.provider = PROVIDER.to_owned();
    handle.metadata = None;
    assert!(
        client
            .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
            .await
            .is_err()
    );
    let prompt = crate::protocol::prompt::AgentPrompt {
        text: "image".to_owned(),
        images: vec![crate::protocol::prompt::PromptImage {
            data: "aGVsbG8=".to_owned(),
            mime_type: "image/png".to_owned(),
        }],
        ..crate::protocol::prompt::AgentPrompt::default()
    };
    assert!(session.start_input(&prompt, &spec.config).await.is_err());
    session.close().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn environment_is_ephemeral_and_catalog_has_a_user_facing_label() {
    let (directory, client, spec) = fixture();
    let environment = AgentEnvironment::try_from(BTreeMap::from([
        (
            "ACP_FIXTURE_LOG".to_owned(),
            directory
                .path()
                .join("requests.jsonl")
                .to_str()
                .unwrap()
                .to_owned(),
        ),
        ("ACP_TEST_ENV".to_owned(), "test-only-value".to_owned()),
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
        requests(&directory)
            .iter()
            .all(|request| request["hasEnvironment"] == true)
    );
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
    assert_eq!(snapshot["entries"][0]["label"], "DeepSeek Harness");
    assert_eq!(snapshot["entries"][0]["status"], "ready");
}

#[cfg(unix)]
#[tokio::test]
async fn bad_protocol_frames_and_timeout_fail_without_hanging() {
    for scenario in ["bad-version", "malformed", "oversized", "hung"] {
        let (_directory, mut client, spec) = fixture();
        client.deadline = Duration::from_millis(300);
        client.environment = AgentEnvironment::try_from(BTreeMap::from([(
            "ACP_FIXTURE_SCENARIO".to_owned(),
            scenario.to_owned(),
        )]))
        .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(3), client.create_session(&spec))
                .await
                .unwrap()
                .is_err(),
            "{scenario}"
        );
    }
}

#[tokio::test]
async fn image_input_is_capability_gated_and_foreign_session_updates_fail() {
    let (directory, mut client, spec) = fixture();
    client.environment = AgentEnvironment::try_from(BTreeMap::from([
        (
            "ACP_FIXTURE_SCENARIO".to_owned(),
            "image-enabled".to_owned(),
        ),
        (
            "ACP_FIXTURE_LOG".to_owned(),
            directory
                .path()
                .join("requests.jsonl")
                .to_str()
                .unwrap()
                .to_owned(),
        ),
    ]))
    .unwrap();
    let mut session = client.create_session(&spec).await.unwrap();
    let prompt = crate::protocol::prompt::AgentPrompt {
        text: "wait".to_owned(),
        images: vec![crate::protocol::prompt::PromptImage {
            data: "aGVsbG8=".to_owned(),
            mime_type: "image/png".to_owned(),
        }],
        ..crate::protocol::prompt::AgentPrompt::default()
    };
    let turn = session.start_input(&prompt, &spec.config).await.unwrap();
    session.cancel_turn(&turn).await.unwrap();
    while event(session.as_mut()).await != AgentTurnEvent::Cancelled {}
    session.close().await.unwrap();
    let logged = requests(&directory);
    let input = logged
        .iter()
        .find(|request| request["method"] == "session/prompt")
        .unwrap();
    assert_eq!(
        input["params"]["prompt"][1],
        json!({"type":"image","mimeType":"image/png","data":"aGVsbG8="})
    );

    client.environment = AgentEnvironment::try_from(BTreeMap::from([(
        "ACP_FIXTURE_SCENARIO".to_owned(),
        "wrong-session".to_owned(),
    )]))
    .unwrap();
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

async fn assert_resume_and_cancel(
    client: &DeepSeekHarnessClient,
    spec: &AgentSessionSpec,
    handle: &AgentPersistenceHandle,
    directory: &TempDir,
) {
    assert!(
        client
            .resume_session(handle, spec, AgentResumePurpose::History)
            .await
            .is_err()
    );
    let mut resumed = client
        .resume_session(handle, spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    let turn = resumed.start_turn("wait", &spec.config).await.unwrap();
    assert!(resumed.cancel_turn("other-turn").await.is_err());
    resumed.cancel_turn(&turn).await.unwrap();
    loop {
        if event(resumed.as_mut()).await == AgentTurnEvent::Cancelled {
            break;
        }
    }
    resumed.close().await.unwrap();
    let logged = requests(directory);
    assert!(
        logged
            .iter()
            .any(|request| request["method"] == "session/resume")
    );
    assert!(
        !logged
            .iter()
            .any(|request| request["method"] == "session/load")
    );
}
