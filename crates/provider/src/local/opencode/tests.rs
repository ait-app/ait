use std::{collections::BTreeMap, time::Duration};

use domain::agent_runtime::StoredAgentConfig;
use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;
use crate::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionSpec,
    AgentTurnEvent,
};

#[cfg(unix)]
fn fixture(scenario: &str) -> (TempDir, OpenCodeClient, AgentSessionSpec) {
    let root = tempfile::tempdir().unwrap();
    let cwd = root
        .path()
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let binary =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/opencode_acp.py");
    let mut client = OpenCodeClient::new(binary);
    client.environment = BTreeMap::from([
        ("AIT_ACP_FIXTURE_ROOT".into(), cwd.clone()),
        ("AIT_ACP_SCENARIO".into(), scenario.into()),
    ]);
    let spec = AgentSessionSpec {
        provider: "opencode".into(),
        cwd,
        config: StoredAgentConfig {
            model: Some("local/model".into()),
            mode_id: Some("build".into()),
            ..StoredAgentConfig::default()
        },
    };
    (root, client, spec)
}

fn requests(root: &TempDir) -> Vec<Value> {
    std::fs::read_to_string(root.path().join("requests.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

async fn event(session: &mut dyn AgentSession) -> AgentTurnEvent {
    tokio::time::timeout(Duration::from_secs(10), async {
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

async fn completed(session: &mut dyn AgentSession) -> Vec<crate::protocol::timeline::NativeItem> {
    let mut items = Vec::new();
    loop {
        match event(session).await {
            AgentTurnEvent::Timeline(entry) => items.push(entry),
            AgentTurnEvent::Completed(text) => {
                assert_eq!(text.as_deref(), Some("authoritative answer"));
                return items;
            }
            AgentTurnEvent::Cancelled | AgentTurnEvent::Failed => {
                panic!("unexpected terminal event")
            }
            _ => {}
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn acp_multiple_turns_replay_and_legacy_resume_keep_native_identity_and_client_ids() {
    let (root, client, spec) = fixture("normal");
    let mut session = client.create_session(&spec).await.unwrap();
    let mut all = Vec::new();
    for index in 0..2 {
        let prompt = crate::protocol::prompt::AgentPrompt {
            text: format!("hello {index}"),
            client_message_id: Some(format!("client-{index}")),
            ..Default::default()
        };
        session.start_input(&prompt, &spec.config).await.unwrap();
        assert_eq!(
            session
                .start_turn("overlap", &spec.config)
                .await
                .unwrap_err(),
            AgentSessionError::Rejected
        );
        all.extend(completed(session.as_mut()).await);
    }
    let handle = session.persistence().unwrap();
    assert_eq!(handle.session_id, "ses_one");
    let saved: Value =
        serde_json::from_str(handle.native_handle.as_ref().unwrap().as_str().unwrap()).unwrap();
    assert_eq!(saved["clients"]["user1"], "client-0");
    assert_eq!(saved["clients"]["user2"], "client-1");
    session.close().await.unwrap();
    let listed = client
        .list_sessions(&crate::ports::native_history::ListOptions {
            cwd: Some(spec.cwd.clone()),
            scan_limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(listed[0].first_prompt_preview.as_deref(), Some("hello 0"));
    assert_eq!(listed[0].last_prompt_preview.as_deref(), Some("hello 1"));
    let before = std::fs::read(root.path().join("native-fixture.json")).unwrap();
    assert_replay(&all, &client.history(&handle, &spec.cwd).await.unwrap());
    assert_eq!(
        std::fs::read(root.path().join("native-fixture.json")).unwrap(),
        before
    );
    let mut legacy = handle.clone();
    legacy.native_handle =
        Some(json!({"config":spec.config,"model":"local/model","clients":saved["clients"]}));
    let mut resumed = client
        .resume_session(&legacy, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    resumed
        .start_turn("after restart", &spec.config)
        .await
        .unwrap();
    assert_eq!(completed(resumed.as_mut()).await.len(), 2);
    resumed.close().await.unwrap();
    assert_eq!(
        requests(&root)
            .iter()
            .filter(|request| request["method"] == "session/prompt")
            .count(),
        3
    );
}

#[cfg(unix)]
#[tokio::test]
async fn acp_replays_more_than_the_control_queue_without_truncating_history() {
    let (root, client, spec) = fixture("normal");
    let history = (0..160).flat_map(|index| [
        json!({"id":format!("user{index}"),"type":"user","text":format!("user {index}")}),
        json!({"id":format!("answer{index}"),"type":"assistant","content":[{"type":"text","text":format!("answer {index}")}]}),
    ]).collect::<Vec<_>>();
    std::fs::write(
        root.path().join("native-fixture.json"),
        json!({"seq":160,"history":history}).to_string(),
    )
    .unwrap();
    let handle = domain::agent_runtime::AgentPersistenceHandle {
        provider: "opencode".into(),
        session_id: "ses_one".into(),
        native_handle: None,
        metadata: None,
    };
    let imported = client.inspect_session(&handle, &spec.cwd).await.unwrap();
    assert_eq!(imported.entries.len(), 320);
    assert_eq!(
        imported.descriptor.first_prompt_preview.as_deref(),
        Some("user 0")
    );
    assert_eq!(
        imported.descriptor.last_prompt_preview.as_deref(),
        Some("user 159")
    );
    assert_eq!(imported.entries.last().unwrap().item["text"], "answer 159");
    assert!(
        requests(&root)
            .iter()
            .all(|request| request["method"] != "session/prompt")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn acp_missing_preview_replay_keeps_discovered_sessions_without_prompting() {
    let (root, client, spec) = fixture("preview-failed");
    std::fs::write(
        root.path().join("native-fixture.json"),
        r#"{"seq":0,"history":[]}"#,
    )
    .unwrap();
    let listed = client
        .list_sessions(&crate::ports::native_history::ListOptions {
            cwd: Some(spec.cwd),
            scan_limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].provider_handle_id, "ses_one");
    assert!(listed[0].first_prompt_preview.is_none());
    assert!(
        requests(&root)
            .iter()
            .all(|request| request["method"] != "session/prompt"
                && request["method"] != "session/new"
                && request["method"] != "session/set_config_option")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn acp_forms_keep_multiselect_values_custom_fields_and_single_reply() {
    let (root, client, spec) = fixture("question");
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("ask", &spec.config).await.unwrap();
    let request = loop {
        if let AgentTurnEvent::PermissionRequested(request) = event(session.as_mut()).await {
            break request;
        }
    };
    assert_eq!(request["kind"], "question");
    assert_eq!(
        request["input"]["questions"][0]["options"][0]["label"],
        "Rust, stable"
    );
    assert_eq!(session.pending_permissions().len(), 1);
    let id = request["id"].as_str().unwrap();
    assert_eq!(
        session
            .respond_permission(
                id,
                &json!({"behavior":"allow","updatedInput":{"answers":{"language":["invalid"]}}})
            )
            .await
            .unwrap_err(),
        AgentSessionError::Rejected
    );
    assert_eq!(
        session
            .respond_permission(
                id,
                &json!({"behavior":"allow","updatedInput":{"content":{"language":["rust","rust"]}}})
            )
            .await
            .unwrap_err(),
        AgentSessionError::Rejected
    );
    session.respond_permission(id, &json!({"behavior":"allow","updatedInput":{"answers":{"language":["Rust, stable","Go"],"language_custom":"C++"}}})).await.unwrap();
    assert!(session.pending_permissions().is_empty());
    assert!(
        session
            .respond_permission(id, &json!({"behavior":"allow"}))
            .await
            .is_err()
    );
    completed(session.as_mut()).await;
    let replies = requests(&root)
        .into_iter()
        .filter(|message| message["id"] == "question" && message.get("method").is_none())
        .collect::<Vec<_>>();
    assert_eq!(replies.len(), 1);
    assert_eq!(
        replies[0]["result"],
        json!({"action":"accept","content":{"language":["rust","go"],"language_custom":"C++"}})
    );
    session.close().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn acp_declining_a_form_continues_the_turn_without_interrupting_it() {
    let (root, client, spec) = fixture("question");
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("ask", &spec.config).await.unwrap();
    loop {
        if let AgentTurnEvent::PermissionRequested(request) = event(session.as_mut()).await {
            session
                .respond_permission(request["id"].as_str().unwrap(), &json!({"behavior":"deny"}))
                .await
                .unwrap();
            break;
        }
    }
    completed(session.as_mut()).await;
    assert!(
        !requests(&root)
            .iter()
            .any(|request| request["method"] == "session/cancel")
    );
    session.close().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn acp_native_permission_options_and_rejection_allow_a_followup_turn() {
    let (root, client, spec) = fixture("permission");
    let mut session = client.create_session(&spec).await.unwrap();
    for behavior in ["deny", "allow"] {
        session.start_turn("shell", &spec.config).await.unwrap();
        loop {
            if let AgentTurnEvent::PermissionRequested(request) = event(session.as_mut()).await {
                session
                    .respond_permission(
                        request["id"].as_str().unwrap(),
                        &json!({"behavior":behavior}),
                    )
                    .await
                    .unwrap();
                break;
            }
        }
        completed(session.as_mut()).await;
    }
    let replies = requests(&root)
        .into_iter()
        .filter(|message| message["id"] == "permission" && message.get("method").is_none())
        .collect::<Vec<_>>();
    assert_eq!(replies[0]["result"]["outcome"]["optionId"], "reject");
    assert_eq!(replies[1]["result"]["outcome"]["optionId"], "once");
    session.close().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn acp_cancel_waits_for_settlement_and_clears_pending_questions() {
    let (_root, client, spec) = fixture("question");
    let mut session = client.create_session(&spec).await.unwrap();
    let turn = session.start_turn("ask", &spec.config).await.unwrap();
    let request = loop {
        if let AgentTurnEvent::PermissionRequested(request) = event(session.as_mut()).await {
            break request;
        }
    };
    session.cancel_turn(&turn).await.unwrap();
    assert_eq!(
        session
            .start_turn("too early", &spec.config)
            .await
            .unwrap_err(),
        AgentSessionError::Rejected
    );
    loop {
        if matches!(event(session.as_mut()).await, AgentTurnEvent::Cancelled) {
            break;
        }
    }
    assert!(session.pending_permissions().is_empty());
    assert!(
        session
            .respond_permission(request["id"].as_str().unwrap(), &json!({"behavior":"deny"}))
            .await
            .is_err()
    );
    session.start_turn("next", &spec.config).await.unwrap();
    session.close().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn acp_one_megabyte_tool_output_stays_in_native_storage_with_bounded_timeline() {
    let (root, client, spec) = fixture("large-tool");
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("read", &spec.config).await.unwrap();
    let items = completed(session.as_mut()).await;
    let tool = items
        .iter()
        .find(|entry| entry.item["type"] == "tool_call")
        .unwrap();
    assert!(serde_json::to_vec(tool).unwrap().len() < 768 * 1024);
    assert!(
        tool.item["detail"]["content"]
            .as_str()
            .unwrap()
            .contains("Output truncated")
    );
    let native: Value =
        serde_json::from_slice(&std::fs::read(root.path().join("native-fixture.json")).unwrap())
            .unwrap();
    assert_eq!(
        native["history"][1]["content"][0]["output"]
            .as_str()
            .unwrap()
            .len(),
        1_048_576
    );
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    assert_replay(&items, &client.history(&handle, &spec.cwd).await.unwrap());
}

#[cfg(unix)]
#[tokio::test]
async fn acp_discovery_never_prompts_and_deletes_its_native_query_session() {
    let (root, client, spec) = fixture("normal");
    let details = client.discover(&spec.cwd).await.unwrap();
    assert_eq!(details.models[0]["id"], "local/model");
    assert!(
        client
            .list_sessions(&crate::ports::native_history::ListOptions {
                cwd: Some(spec.cwd),
                scan_limit: 10
            })
            .await
            .unwrap()
            .is_empty()
    );
    assert!(!root.path().join("native-fixture.json").exists());
    assert!(
        !requests(&root)
            .iter()
            .any(|request| request["method"] == "session/prompt")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn acp_ambiguous_admission_is_never_resubmitted() {
    let (root, client, spec) = fixture("ambiguous");
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("once", &spec.config).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if session.poll_turn().is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(session.start_turn("retry", &spec.config).await.is_err());
    assert_eq!(
        requests(&root)
            .iter()
            .filter(|message| message["method"] == "session/prompt")
            .count(),
        1
    );
    session.close().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn acp_malformed_initialization_and_timeouts_fail_closed() {
    for scenario in ["malformed", "hung"] {
        let (_root, mut client, spec) = fixture(scenario);
        client.deadline = Duration::from_millis(150);
        assert!(client.create_session(&spec).await.is_err());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn acp_concurrent_immutable_executables_are_isolated() {
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..32 {
        tasks.spawn(async {
            let (_root, client, spec) = fixture("normal");
            let mut session = client.create_session(&spec).await.unwrap();
            session
                .start_turn("concurrent", &spec.config)
                .await
                .unwrap();
            completed(session.as_mut()).await;
            session.close().await.unwrap();
        });
    }
    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }
}

#[test]
fn acp_configuration_rejects_unsupported_policy_before_launch() {
    for config in [
        StoredAgentConfig {
            mode_id: Some("bad\nmode".into()),
            ..Default::default()
        },
        StoredAgentConfig {
            model: Some("missing-provider".into()),
            ..Default::default()
        },
        StoredAgentConfig {
            mcp_servers: Some(BTreeMap::new()),
            ..Default::default()
        },
    ] {
        assert_eq!(config::validate(&config), Err(AgentSessionError::Rejected));
    }
}

#[cfg(unix)]
mod installed;

fn assert_replay(
    items: &[crate::protocol::timeline::NativeItem],
    replay: &[crate::protocol::timeline::NativeItem],
) {
    let timeline = crate::storage::timeline::Timeline::memory().unwrap();
    let (epoch, _) = timeline.append("agent", "opencode", items).unwrap();
    let (_, before) = timeline.read("agent").unwrap();
    assert_eq!(
        timeline.reconcile("agent", "opencode", replay).unwrap(),
        epoch
    );
    let (_, after) = timeline.read("agent").unwrap();
    assert_eq!(
        before
            .iter()
            .map(crate::storage::timeline::Row::value)
            .collect::<Vec<_>>(),
        after
            .iter()
            .map(crate::storage::timeline::Row::value)
            .collect::<Vec<_>>()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn acp_history_only_sessions_reject_writes_and_keep_native_storage_unchanged() {
    let (root, client, spec) = fixture("normal");
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("initial", &spec.config).await.unwrap();
    completed(session.as_mut()).await;
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let original = std::fs::read(root.path().join("native-fixture.json")).unwrap();
    let mut historical = client
        .resume_session(&handle, &spec, AgentResumePurpose::History)
        .await
        .unwrap();
    assert_eq!(
        historical
            .start_turn("write", &spec.config)
            .await
            .unwrap_err(),
        AgentSessionError::Rejected
    );
    historical.close().await.unwrap();
    assert_eq!(
        std::fs::read(root.path().join("native-fixture.json")).unwrap(),
        original
    );
}

#[cfg(unix)]
#[tokio::test]
async fn acp_dynamic_modes_effort_and_permission_overrides_do_not_create_a_new_session() {
    let (root, client, mut spec) = fixture("normal");
    let mut session = client.create_session(&spec).await.unwrap();
    spec.config.mode_id = Some("plan".into());
    spec.config.thinking_option_id = Some("high".into());
    spec.config.feature_values = Some(BTreeMap::from([("permission".into(), json!("deny"))]));
    session.start_turn("plan", &spec.config).await.unwrap();
    completed(session.as_mut()).await;
    let info = session.runtime_info().await.unwrap();
    assert_eq!(info.mode_id.as_deref(), Some("plan"));
    assert_eq!(info.thinking_option_id.as_deref(), Some("high"));
    assert_eq!(
        requests(&root)
            .iter()
            .filter(|message| message["method"] == "session/new")
            .count(),
        1
    );
    assert!(
        requests(&root)
            .iter()
            .any(|message| message["method"] == "session/resume")
    );
    session.close().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn acp_auxiliary_generation_cleans_up_on_success_and_future_cancellation() {
    let (root, client, spec) = fixture("normal");
    assert_eq!(
        client
            .generate_summary(&spec, "metadata", &json!({"type":"object"}))
            .await
            .unwrap(),
        "authoritative answer"
    );
    assert!(!root.path().join("native-fixture.json").exists());
    let (root, client, spec) = fixture("waiting");
    let task = tokio::spawn(async move {
        client
            .generate_summary(&spec, "metadata", &json!({"type":"object"}))
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if root.path().join("requests.jsonl").exists()
                && requests(&root)
                    .iter()
                    .any(|request| request["method"] == "session/prompt")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    task.abort();
    let _ = task.await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while root.path().join("native-fixture.json").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn acp_refuses_versions_without_native_form_support_before_creating_sessions() {
    for version in ["1.18.4", "2.0.25", "not-a-version"] {
        let (root, mut client, spec) = fixture("normal");
        client
            .environment
            .insert("AIT_ACP_VERSION".into(), version.into());
        assert!(client.create_session(&spec).await.is_err());
        assert!(!root.path().join("native-fixture.json").exists());
    }
}
