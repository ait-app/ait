use std::collections::BTreeMap;
use std::time::Duration;

use super::*;
use provider::ports::agent_session::AgentTurnEvent;
use provider::protocol::prompt::{AgentPrompt, PromptImage};

mod capabilities;
mod commands;
mod diagnostics;
mod installed;
mod interactions;
mod native_history;
mod real;
mod settlement;
mod transport;

#[tokio::test]
async fn an_explicit_program_is_used_without_falling_back_to_path() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("omp");
    let program = installed_program(|name| {
        (name == "AIT_SERVER_OMP_BIN").then(|| missing.clone().into_os_string())
    });

    assert_eq!(program, missing);
    assert!(!OmpClient::new(program).is_available().await.unwrap());
    assert_eq!(installed_program(|_| None), PathBuf::from("omp"));
}

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
    use domain::agent_runtime::registry::AgentRuntimeRegistry;
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
    use provider::service::agent_manager::AgentManager;
    use provider::storage::timeline::Timeline;
    use provider::testing::Manager;
    let (root, client, spec) = fixture();
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    let timeline = Timeline::open(&root.path().join("timeline.sqlite")).unwrap();
    let mut manager =
        Manager::new(AgentManager::new(Box::new(registry.clone())).with_timeline(timeline.clone()));
    manager.register(Box::new(client.clone())).unwrap();
    manager.create("agent", &spec).await.unwrap();
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
    let (epoch, rows) = provider::testing::timeline_entries(&timeline, "agent").unwrap();
    assert!(rows.iter().any(|row| row.item["text"] == "Hello world"));
    assert_eq!(
        provider::testing::timeline_subagents(&timeline, "agent")
            .unwrap()
            .len(),
        1
    );
    manager.close_all().await.unwrap();
    let mut manager =
        Manager::new(AgentManager::new(Box::new(registry)).with_timeline(timeline.clone()));
    manager.register(Box::new(client)).unwrap();
    manager.resume("agent").await.unwrap();
    assert_eq!(
        provider::testing::timeline_entries(&timeline, "agent")
            .unwrap()
            .0,
        epoch
    );
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

#[cfg(unix)]
#[tokio::test]
async fn symlinked_workspaces_match_the_physical_cwd_recorded_by_omp() {
    let (root, client, mut spec) = fixture();
    let project = root.path().join("project");
    let linked = root.path().join("linked-project");
    let other = root.path().join("other-project");
    std::fs::create_dir(&project).unwrap();
    std::fs::create_dir(&other).unwrap();
    std::os::unix::fs::symlink(&project, &linked).unwrap();
    spec.cwd = linked.to_str().unwrap().to_owned();
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("hello", &spec.config).await.unwrap();
    complete(session.as_mut()).await;
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let listing = |cwd: &str| ListOptions {
        cwd: Some(cwd.to_owned()),
        scan_limit: 10,
    };

    let history = client.history(&handle, &spec.cwd).await.unwrap();
    let sessions = client.list_sessions(&listing(&spec.cwd)).await.unwrap();

    assert!(history.iter().any(|entry| entry.item["text"] == "hello"));
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].provider_handle_id, handle.session_id);
    let other = other.to_str().unwrap();
    assert!(client.history(&handle, other).await.is_err());
    assert!(
        client
            .list_sessions(&listing(other))
            .await
            .unwrap()
            .is_empty()
    );
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

/// Create an isolated offline native peer, client and default session specification.
pub(super) fn fixture() -> (tempfile::TempDir, OmpClient, AgentSessionSpec) {
    let root = tempfile::tempdir().unwrap();
    let program =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/fixtures/omp.py");
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
        confinement: None,
    };
    (root, client, spec)
}

/// Confinement for an Agent whose worktree is `root`; `root` must hold a `.git` directory
/// for the sandbox policy to resolve.
pub(super) fn confinement(
    root: &std::path::Path,
) -> Option<domain::workspace::sandbox::WriteConfinement> {
    let workspace = serde_json::from_value(json!({
        "workspaceId": "wks_00000000000000a1", "projectId": "prj_1", "kind": "worktree",
        "cwd": root, "worktreeRoot": root, "displayName": "sandbox",
        "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-01T00:00:00Z",
        "archivedAt": null, "sandbox": "worktree_writes"
    }))
    .unwrap();
    domain::workspace::sandbox::WriteConfinement::for_workspace(&workspace)
}

/// Create the offline fixture with extra native `variables` selecting fixture behaviour.
pub(super) fn fixture_with(
    variables: &[(&str, &str)],
) -> (tempfile::TempDir, OmpClient, AgentSessionSpec) {
    let (root, client, spec) = fixture();
    (root, with_variables(&client, variables), spec)
}

/// Return `client` with extra native `variables` selecting fixture behaviour.
pub(super) fn with_variables(client: &OmpClient, variables: &[(&str, &str)]) -> OmpClient {
    let mut values: BTreeMap<String, String> = client
        .environment
        .entries()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    values.extend(
        variables
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned())),
    );
    OmpClient {
        environment: AgentEnvironment::try_from(values).unwrap(),
        ..client.clone()
    }
}

/// Return the native calls the fixture logged after the first `skip` lines.
pub(super) fn calls_after(root: &std::path::Path, skip: usize) -> Vec<Value> {
    std::fs::read_to_string(root.join("agent/calls.jsonl"))
        .unwrap()
        .lines()
        .skip(skip)
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// Return the argv of every OMP process the fixture logged.
pub(super) fn launches(calls: &[Value]) -> Vec<Vec<String>> {
    calls
        .iter()
        .filter_map(|call| serde_json::from_value(call.get("args")?.clone()).ok())
        .collect()
}

/// Count the logged native commands of `kind`.
pub(super) fn count(calls: &[Value], kind: &str) -> usize {
    calls.iter().filter(|call| call["type"] == kind).count()
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
    assert_eq!(details.models.len(), 4);
    assert_ne!(details.models[0]["id"], details.models[1]["id"]);
    // Paseo's built-ins come first, then OMP's own commands (D25).
    let commands = client.commands(&spec).await.unwrap();
    let names: Vec<_> = commands.iter().map(|command| &command["name"]).collect();
    assert_eq!(
        names,
        [
            "compact",
            "autocompact",
            "handoff",
            "steer",
            "follow-up",
            "review"
        ]
    );
    assert_eq!(commands[5]["kind"], "skill");
    assert!(client.handles_out_of_band("/compact"));
    assert!(client.handles_out_of_band("/steer go on"));
    assert!(!client.handles_out_of_band("/steer"));
    assert!(!client.handles_out_of_band("/review"));
    assert!(
        client
            .diagnostic()
            .await
            .unwrap()
            .contains("Version: omp/18.18.8")
    );
    assert!(!root.path().join("agent/sessions").exists());
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(!calls.contains("\"type\": \"prompt\""));
}

#[tokio::test]
async fn native_tools_custom_messages_telemetry_and_todos_follow_paseo_and_replay_identically() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("timeline-mapping", &spec.config)
        .await
        .unwrap();
    let events = complete(session.as_mut()).await;
    assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
    let live: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Timeline(entry) => Some(entry.clone()),
            _ => None,
        })
        .collect();
    let row = |predicate: &dyn Fn(&Value) -> bool| {
        live.iter()
            .find(|entry| predicate(&entry.item))
            .unwrap_or_else(|| panic!("missing row in {live:?}"))
            .item
            .clone()
    };
    let named = |name: &'static str| row(&move |item: &Value| item["name"] == name);
    assert_eq!(
        named("edit")["detail"]["unifiedDiff"],
        "-1|line one\n+1|line 1\n 2|line two"
    );
    assert!(
        named("edit")["detail"]["filePath"]
            .as_str()
            .unwrap()
            .ends_with("/probe.txt")
    );
    assert_eq!(
        row(&|item: &Value| item["callId"] == "read-1")["detail"]["content"],
        "line 1"
    );
    let search = named("web_search");
    assert_eq!(search["status"], "failed");
    assert_eq!(search["error"], "All web search providers failed");
    assert_eq!(named("task")["detail"]["subAgentType"], "Prober");
    assert!(live.iter().all(|entry| entry.item["name"] != "todo"));
    let todos: Vec<_> = live
        .iter()
        .filter(|entry| entry.item["type"] == "todo")
        .collect();
    assert_eq!(todos.len(), 2);
    assert_eq!(
        todos[0].item["items"][1]["text"],
        "two (blocked: waiting on api)"
    );
    assert_eq!(todos[1].item["items"].as_array().unwrap().len(), 1);
    assert!(
        row(&|item: &Value| item["type"] == "notification" && item["level"] == "warning")
            ["message"]
            .as_str()
            .unwrap()
            .starts_with("Cost warning")
    );
    assert_eq!(named("omp_goal_updated")["callId"], "omp-goal:goal-1");
    assert_eq!(named("omp_auto_retry")["callId"], "omp-auto-retry:1");
    assert_eq!(
        named("omp_retry_fallback")["detail"]["text"],
        "test/model -> other/model"
    );
    let compactions: Vec<_> = live
        .iter()
        .filter(|entry| entry.item["type"] == "compaction")
        .map(|entry| &entry.item)
        .collect();
    assert_eq!(compactions.len(), 6);
    assert_eq!(compactions[1]["preTokens"], 100_010);
    assert_eq!(compactions[2]["trigger"], "manual");
    assert_eq!(
        row(&|item: &Value| item["type"] == "notification" && item["level"] == "info")["message"],
        "Background job Prober completed\nchild done\nProber is now idle"
    );
    assert_eq!(
        named("advisor")["detail"]["text"],
        "[blocker] [rust] Handle the error"
    );
    assert_eq!(
        named("vendor-note")["detail"]["text"],
        "Plain extension output"
    );
    assert!(
        live.iter()
            .all(|entry| entry.item["name"] != "hidden-context")
    );

    // History replays every recorded row identically; telemetry exists only live.
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let history = client.history(&handle, &spec.cwd).await.unwrap();
    let recorded: Vec<_> = live
        .into_iter()
        .filter(|entry| !entry.key.contains(":notice:"))
        .collect();
    assert_eq!(history, recorded);
}

/// Check that `events` carry unique OMP request segments, all queued before the terminal event.
fn assert_request_usage_precedes_terminal(events: &[AgentTurnEvent]) {
    let requests: Vec<_> = events
        .iter()
        .enumerate()
        .filter_map(|(position, event)| match event {
            AgentTurnEvent::RequestUsage(request) => Some((position, request)),
            _ => None,
        })
        .collect();
    assert!(!requests.is_empty());
    assert!(
        requests
            .iter()
            .all(|(position, _)| *position < events.len() - 1)
    );
    let ids: std::collections::BTreeSet<_> = requests
        .iter()
        .map(|(_, request)| request.segment_id.as_str())
        .collect();
    assert_eq!(ids.len(), requests.len());
    let (_, request) = requests[0];
    assert!(request.segment_id.starts_with("omp:"));
    assert_eq!(request.model.as_deref(), Some("model"));
    assert_eq!(
        (
            request.input_tokens,
            request.output_tokens,
            request.cache_read_tokens
        ),
        (10, 5, 2)
    );
    assert_eq!(request.cost_usd_reported, Some(0.001));
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
    assert!(session.reports_token_usage());
    let events = complete(session.as_mut()).await;
    assert_request_usage_precedes_terminal(&events);
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
    session.start_turn("reject", &spec.config).await.unwrap();
    assert_eq!(
        complete(session.as_mut()).await.last(),
        Some(&AgentTurnEvent::Failed)
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
        Some(AgentTurnEvent::Completed(Some(output))) if output == "Session: local"
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn garbage_output_is_skipped_without_failing_or_resending_input() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("garbage-line", &spec.config)
        .await
        .unwrap();

    assert!(matches!(
        complete(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(Some(reply))) if reply == "Hello world"
    ));
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

#[tokio::test]
async fn local_command_admission_does_not_pretend_native_history_was_persisted() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("/notify-only", &spec.config)
        .await
        .unwrap();
    let events = complete(session.as_mut()).await;
    assert!(
        matches!(events.last(), Some(AgentTurnEvent::Completed(Some(text))) if text == "Notify only output")
    );
    let handle = session.persistence().unwrap();
    assert_eq!(handle.metadata.as_ref().unwrap()["empty"], true);
    session.close().await.unwrap();
    assert!(client.history(&handle, &spec.cwd).await.unwrap().is_empty());
    let mut reopened = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn launch_passes_model_and_thinking_as_flags_in_paseos_order() {
    let (root, client, mut spec) = fixture();
    spec.config.model = Some(json!(["other", "model"]).to_string());
    spec.config.thinking_option_id = Some("max".to_owned());
    spec.config.system_prompt = Some("Be brief.".to_owned());

    let mut session = client.create_session(&spec).await.unwrap();

    let info = session.runtime_info().await.unwrap();
    assert_eq!(info.model, spec.config.model);
    assert_eq!(info.thinking_option_id.as_deref(), Some("max"));
    assert_eq!(info.mode_id.as_deref(), Some("full"));
    session.close().await.unwrap();
    let calls = calls_after(root.path(), 0);
    assert_eq!(
        launches(&calls),
        [[
            "--mode",
            "rpc-ui",
            "--approval-mode",
            "yolo",
            "--model",
            "other/model",
            "--thinking",
            "max",
            "--append-system-prompt",
            "Be brief.",
        ]]
    );
    for kind in [
        "set_model",
        "set_thinking_level",
        "get_available_thinking_levels",
        "get_available_models",
    ] {
        assert_eq!(count(&calls, kind), 0, "{kind}");
    }

    spec.config.thinking_option_id = Some("vendor-defined-effort".to_owned());
    spec.config.model = Some(json!(["missing", "model"]).to_string());
    assert!(client.create_session(&spec).await.is_err());
    let launched = launches(&calls_after(root.path(), calls.len()));
    assert!(launched[0].contains(&"missing/model".to_owned()));
    assert!(!launched[0].contains(&"--thinking".to_owned()));
}

/// Requested model, fixture switches, expected model, and how many `set_model` calls are sent.
type ResumeCase<'a> = (&'a str, &'a [(&'a str, &'a str)], &'a str, usize);

#[tokio::test]
async fn resume_keeps_omps_model_unless_the_requested_model_is_listed() {
    let (root, client, mut spec) = fixture();
    spec.config.model = Some(json!(["other", "model"]).to_string());
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("hello", &spec.config).await.unwrap();
    complete(session.as_mut()).await;
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let file = handle.metadata.as_ref().unwrap()["sessionFile"]
        .as_str()
        .unwrap()
        .to_owned();
    let cases: [ResumeCase<'_>; 4] = [
        ("other", &[], "other", 0),
        ("test", &[], "test", 1),
        (
            "other",
            &[("OMP_FIXTURE_REMOVED_MODEL", "other/model")],
            "test",
            0,
        ),
        ("other", &[("OMP_FIXTURE_SET_MODEL_FAILS", "1")], "test", 1),
    ];
    for (requested, variables, expected, switches) in cases {
        let skip = calls_after(root.path(), 0).len();
        spec.config.model = Some(json!([requested, "model"]).to_string());
        let client = with_variables(&client, variables);

        let mut resumed = client
            .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
            .await
            .unwrap();

        let info = resumed.runtime_info().await.unwrap();
        assert_eq!(info.model, Some(json!([expected, "model"]).to_string()));
        assert_eq!(info.session_id.as_deref(), Some(handle.session_id.as_str()));
        resumed.close().await.unwrap();
        let calls = calls_after(root.path(), skip);
        let launched = launches(&calls);
        assert_eq!(launched.len(), 1);
        assert!(launched[0].ends_with(&["--session".to_owned(), file.clone()]));
        assert!(!launched[0].contains(&"--model".to_owned()));
        assert_eq!(
            count(&calls, "set_model"),
            switches,
            "{requested} {variables:?}"
        );
        assert_eq!(
            count(&calls, "get_available_models"),
            usize::from(requested != "other" || !variables.is_empty()),
            "{requested} {variables:?}"
        );
    }
    // OMP itself refuses to resume once the session's own model is gone; Ait does not mask it.
    let client = with_variables(&client, &[("OMP_FIXTURE_REMOVED_MODEL", "test/model")]);
    assert!(
        client
            .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn resume_keeps_omps_model_when_omp_does_not_list_its_models_in_time() {
    let (root, mut client, mut spec) = fixture();
    spec.config.model = Some(json!(["other", "model"]).to_string());
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("hello", &spec.config).await.unwrap();
    complete(session.as_mut()).await;
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    spec.config.model = Some(json!(["test", "model"]).to_string());
    client.timeouts.request = Duration::from_millis(500);
    let client = with_variables(
        &client,
        &[("OMP_FIXTURE_UNANSWERED", "get_available_models")],
    );
    let skip = calls_after(root.path(), 0).len();

    let mut resumed = tokio::time::timeout(
        Duration::from_secs(5),
        client.resume_session(&handle, &spec, AgentResumePurpose::Interactive),
    )
    .await
    .unwrap()
    .unwrap();

    let info = resumed.runtime_info().await.unwrap();
    assert_eq!(info.model, Some(json!(["other", "model"]).to_string()));
    resumed.start_turn("hello", &spec.config).await.unwrap();
    assert!(matches!(
        complete(resumed.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    resumed.close().await.unwrap();
    let calls = calls_after(root.path(), skip);
    assert_eq!(count(&calls, "get_available_models"), 1);
    assert_eq!(count(&calls, "set_model"), 0);
}

/// Return the display items among `events`.
fn timeline_items(events: &[AgentTurnEvent]) -> Vec<&Value> {
    events
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Timeline(entry) => Some(&entry.item),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn local_commands_settle_with_an_echo_and_clean_output_with_or_without_a_result() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    for (index, command) in ["/local", "/local-result"].into_iter().enumerate() {
        let prompt = AgentPrompt {
            client_message_id: Some(format!("client-{index}")),
            ..AgentPrompt::text(command)
        };
        session.start_input(&prompt, &spec.config).await.unwrap();

        let events = complete(session.as_mut()).await;

        let items = timeline_items(&events);
        assert_eq!(items.len(), 2, "{command}: {events:?}");
        assert_eq!(items[0]["type"], "user_message");
        assert_eq!(items[0]["text"], command);
        assert_eq!(items[0]["clientMessageId"], format!("client-{index}"));
        assert_eq!(items[1]["type"], "assistant_message");
        assert_eq!(items[1]["text"], "Session: local");
        assert_eq!(
            events.last(),
            Some(&AgentTurnEvent::Completed(Some(
                "Session: local".to_owned()
            )))
        );
    }
    session.close().await.unwrap();
    // OMP 18.18.8 sends no result for /local, so native idle state settled it.
    assert!(count(&calls_after(root.path(), 0), "get_state") >= 2);
}

#[tokio::test]
async fn late_prompt_rejections_fail_the_turn_with_omps_reason() {
    for prompt in ["late-reject", "late-reject-result"] {
        let (_root, client, spec) = fixture();
        let mut session = client.create_session(&spec).await.unwrap();
        let input = AgentPrompt {
            client_message_id: Some("client-1".to_owned()),
            ..AgentPrompt::text(prompt)
        };
        session.start_input(&input, &spec.config).await.unwrap();

        let events = complete(session.as_mut()).await;

        assert_eq!(events.last(), Some(&AgentTurnEvent::Failed), "{prompt}");
        let items = timeline_items(&events);
        assert_eq!(items.len(), 1, "{prompt}: {events:?}");
        assert_eq!(items[0]["clientMessageId"], "client-1");
        let message = session.failure_message().unwrap().into_owned();
        assert!(message.contains("No API key found for test."), "{message}");
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn cancel_sends_abort_and_omps_queued_steering_runs_as_its_own_turn() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let cancelled = session
        .start_turn("wait-queue", &spec.config)
        .await
        .unwrap();
    next(session.as_mut()).await.unwrap();
    let steer = AgentPrompt {
        client_message_id: Some("steer-1".to_owned()),
        ..AgentPrompt::text("continue")
    };
    session.steer_input(&cancelled, &steer).await.unwrap();
    session.cancel_turn(&cancelled).await.unwrap();

    let events = complete(session.as_mut()).await;

    assert_eq!(events.last(), Some(&AgentTurnEvent::Cancelled));
    // The runtime must not submit new input before it has seen the queued run's turn.
    assert!(session.pending_foreground());
    let AgentTurnEvent::Started(queued) = next(session.as_mut()).await.unwrap() else {
        panic!("expected OMP's queued run to start a turn");
    };
    assert_ne!(queued, cancelled);
    assert!(!session.pending_foreground());
    let events = complete(session.as_mut()).await;
    let items = timeline_items(&events);
    assert!(
        items.iter().any(|item| item["type"] == "user_message"
            && item["text"] == "continue"
            && item["clientMessageId"] == "steer-1"),
        "{events:?}"
    );
    assert_eq!(
        events.last(),
        Some(&AgentTurnEvent::Completed(Some("Steered reply".to_owned())))
    );
    session.close().await.unwrap();
    let calls = calls_after(root.path(), 0);
    assert_eq!(count(&calls, "abort"), 1);
    assert_eq!(count(&calls, "abort_and_restore_queue"), 0);
}

#[tokio::test]
async fn cancellation_closes_tools_and_children_that_never_finished() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let turn = session
        .start_turn("tool-abort", &spec.config)
        .await
        .unwrap();
    loop {
        if let AgentTurnEvent::Subagent(_) = next(session.as_mut()).await.unwrap() {
            break;
        }
    }
    session.cancel_turn(&turn).await.unwrap();

    let events = complete(session.as_mut()).await;

    assert_eq!(events.last(), Some(&AgentTurnEvent::Cancelled));
    assert!(
        timeline_items(&events)
            .iter()
            .any(|item| item["callId"] == "stuck-tool" && item["status"] == "canceled")
    );
    assert!(events.iter().any(|event| matches!(event,
        AgentTurnEvent::Subagent(provider::ports::controls::SubagentEvent::Upsert(child))
            if child.id.ends_with("/worker") && child.descriptor["status"] == "canceled")));
    assert_eq!(session.subagents()[0].descriptor["status"], "canceled");
    session.close().await.unwrap();
}

#[tokio::test]
async fn a_resumed_process_replaying_its_session_shows_only_new_work() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("hello", &spec.config).await.unwrap();
    complete(session.as_mut()).await;
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let client = with_variables(&client, &[("OMP_FIXTURE_REPLAY", "1")]);
    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(resumed.poll_turn().unwrap(), None);

    resumed.start_turn("again", &spec.config).await.unwrap();
    let events = complete(resumed.as_mut()).await;

    let texts: Vec<_> = timeline_items(&events)
        .into_iter()
        .filter_map(|item| item["text"].as_str())
        .collect();
    assert!(texts.contains(&"again"), "{texts:?}");
    assert!(!texts.contains(&"hello"), "{texts:?}");
    resumed.close().await.unwrap();
}

#[tokio::test]
async fn steered_slash_commands_never_reach_omp_and_late_steering_refusals_keep_the_turn() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let turn = session
        .start_turn("steer-reject", &spec.config)
        .await
        .unwrap();
    next(session.as_mut()).await.unwrap();

    assert_eq!(
        session.steer_turn(&turn, " /compact now").await,
        Err(AgentSessionError::Rejected)
    );
    assert_eq!(count(&calls_after(root.path(), 0), "steer"), 0);
    session.steer_turn(&turn, "keep going").await.unwrap();
    let events = complete(session.as_mut()).await;

    assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
    let notices: Vec<_> = timeline_items(&events)
        .into_iter()
        .filter(|item| item["type"] == "notification")
        .collect();
    assert_eq!(notices.len(), 1, "{events:?}");
    assert!(
        !notices[0]["message"]
            .as_str()
            .unwrap()
            .contains("fixture-secret")
    );
    assert_eq!(session.failure_message(), None);
    session.close().await.unwrap();
}

/// The user messages among `events`' timeline items.
fn user_messages(events: &[AgentTurnEvent]) -> Vec<provider::protocol::timeline::NativeItem> {
    events
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Timeline(entry) if entry.item["type"] == "user_message" => {
                Some(entry.clone())
            }
            _ => None,
        })
        .collect()
}

/// Native entry ids of the recorded user messages in the session file behind `handle`.
fn recorded_user_ids(handle: &AgentPersistenceHandle) -> Vec<String> {
    let path = handle.metadata.as_ref().unwrap()["sessionFile"]
        .as_str()
        .unwrap()
        .to_owned();
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|record| record["type"] == "message" && record["message"]["role"] == "user")
        .map(|record| record["id"].as_str().unwrap().to_owned())
        .collect()
}

fn input(text: &str, client: &str) -> AgentPrompt {
    AgentPrompt {
        client_message_id: Some(client.to_owned()),
        ..AgentPrompt::text(text)
    }
}

#[tokio::test]
async fn user_messages_are_named_by_native_entries_shown_once_and_match_history() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_input(&input("duplicate-user", "c1"), &spec.config)
        .await
        .unwrap();
    let mut live = user_messages(&complete(session.as_mut()).await);
    let turn = session
        .start_input(&input("/skill:review wait", "c2"), &spec.config)
        .await
        .unwrap();
    session
        .steer_input(&turn, &input("follow up", "c3"))
        .await
        .unwrap();
    live.extend(user_messages(&complete(session.as_mut()).await));
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();

    let ids = recorded_user_ids(&handle);
    assert_eq!(ids.len(), 2);
    let shown: Vec<_> = live
        .iter()
        .map(|entry| {
            (
                entry.item["text"].clone(),
                entry.item["messageId"].clone(),
                entry.item["clientMessageId"].clone(),
            )
        })
        .collect();
    // The re-announced message shows once; the skill prompt consumes its own input, so the
    // steered message keeps its client identity (D9).
    assert_eq!(
        shown,
        [
            (json!("duplicate-user"), json!(ids[0]), json!("c1")),
            (json!("/skill:review wait"), json!(live[1].key), json!("c2")),
            (json!("follow up"), json!(ids[1]), json!("c3")),
        ]
    );
    let history = client.history(&handle, &spec.cwd).await.unwrap();
    let replayed: Vec<_> = history
        .into_iter()
        .filter(|entry| entry.item["type"] == "user_message")
        .collect();
    assert_eq!(replayed, live);
    let calls = calls_after(root.path(), 0);
    assert_eq!(count(&calls, "get_branch_messages"), 2);
}

#[tokio::test]
async fn entry_ids_on_announced_messages_need_no_lookup() {
    let (root, client, spec) = fixture_with(&[("OMP_FIXTURE_MESSAGE_IDS", "1")]);
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_input(&input("duplicate-user", "c1"), &spec.config)
        .await
        .unwrap();

    let live = user_messages(&complete(session.as_mut()).await);

    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    assert_eq!(live.len(), 1);
    assert_eq!(
        live[0].item["messageId"],
        json!(recorded_user_ids(&handle)[0])
    );
    assert_eq!(live[0].item["clientMessageId"], "c1");
    assert_eq!(
        count(&calls_after(root.path(), 0), "get_branch_messages"),
        0
    );
}
