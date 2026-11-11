use super::*;
use crate::ports::agent_session::AgentClient;
use crate::service::agent_execution::operation::PermissionContext;

#[tokio::test]
async fn startup_dialogs_publish_before_admission_and_do_not_replay_a_cancelled_input() {
    let (root, client, spec) = crate::local::omp::tests::fixture();
    std::fs::create_dir_all(root.path().join("agent")).unwrap();
    std::fs::write(root.path().join("agent/startup-dialog"), "true").unwrap();
    let mut session = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client.create_session(&spec),
    )
    .await
    .expect("startup dialog must not block session publication")
    .unwrap();
    let request = session
        .pending_permissions()
        .into_iter()
        .next()
        .expect("startup request");
    let turn = session
        .start_turn("must not be replayed", &spec.config)
        .await
        .unwrap();
    session.cancel_turn(&turn).await.unwrap();
    assert!(session.pending_permissions().is_empty());
    assert!(session.start_turn("later", &spec.config).await.is_err());
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(!calls.contains("must not be replayed"));
    assert!(request["id"].as_str().is_some());
}

#[tokio::test]
async fn startup_empty_answer_and_cancel_remain_distinct_and_release_one_pending_prompt() {
    for answer in [
        serde_json::json!({"behavior":"allow","updatedInput":{"answers":{"value":""}}}),
        serde_json::json!({"behavior":"deny"}),
    ] {
        let (root, client, spec) = crate::local::omp::tests::fixture();
        std::fs::create_dir_all(root.path().join("agent")).unwrap();
        std::fs::write(root.path().join("agent/startup-dialog"), "true").unwrap();
        let mut session = client.create_session(&spec).await.unwrap();
        let request = session.pending_permissions().pop().unwrap();
        let turn = session.start_turn("Only once", &spec.config).await.unwrap();
        session
            .respond_permission(request["id"].as_str().unwrap(), &answer)
            .await
            .unwrap();
        let events = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let mut events = Vec::new();
            loop {
                session.service_realtime().await.unwrap();
                while let Some(event) = session.poll_turn().unwrap() {
                    let done = matches!(event, AgentTurnEvent::Completed(_));
                    events.push(event);
                    if done {
                        return events;
                    }
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(session.pending_permissions().is_empty());
        assert!(
            events.iter().any(|event| matches!(event,
            AgentTurnEvent::Timeline(entry) if entry.item["text"] == "Only once"
                && entry.turn_id.as_deref() == Some(turn.as_str()))),
            "{events:?}"
        );
        let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
        let calls: Vec<Value> = calls
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let replies: Vec<_> = calls
            .iter()
            .filter(|call| call["type"] == "extension_ui_response")
            .collect();
        assert_eq!(replies.len(), 1);
        if answer["behavior"] == "allow" {
            assert_eq!(replies[0]["value"], "");
            assert!(replies[0].get("cancelled").is_none());
        } else {
            assert_eq!(replies[0]["cancelled"], true);
            assert!(replies[0].get("value").is_none());
        }
        assert_eq!(
            calls.iter().filter(|call| call["type"] == "prompt").count(),
            1
        );
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn manager_publishes_a_startup_question_before_the_first_input_is_admitted() {
    use crate::service::agent_manager::{AgentManager, AgentRegistration};
    use crate::storage::timeline::Timeline;
    let (root, client, spec) = crate::local::omp::tests::fixture();
    std::fs::create_dir_all(root.path().join("agent")).unwrap();
    std::fs::write(root.path().join("agent/startup-dialog"), "true").unwrap();
    let registry = persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry::new(
        root.path().join("agents.json"),
    );
    let timeline = Timeline::open(&root.path().join("timeline.sqlite")).unwrap();
    let mut manager = AgentManager::new(Box::new(registry)).with_timeline(timeline.clone());
    manager.register_client(Box::new(client)).unwrap();
    let record = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        manager.create("startup-agent", &spec, AgentRegistration::default()),
    )
    .await
    .unwrap()
    .unwrap();
    let mut snapshot = serde_json::json!({});
    manager.control_snapshot(&record, &mut snapshot);
    assert_eq!(snapshot["pendingPermissions"][0]["id"], "startup-choice");
    assert!(record.persistence.is_none());
    manager
        .send("startup-agent", "Only after the user answers")
        .await
        .unwrap();
    manager
        .permission(
            "startup-agent",
            "startup-choice",
            &serde_json::json!({"behavior":"deny"}),
            PermissionContext::default(),
        )
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while manager.active_turn("startup-agent").is_some() {
            manager.poll().await.unwrap();
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(
        manager
            .live_snapshot("startup-agent")
            .unwrap()
            .persistence
            .is_some()
    );
    let (_, entries) = timeline.read("startup-agent").unwrap();
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.entry.item["text"] == "Only after the user answers")
            .count(),
        1
    );
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn metadata_reads_keep_project_commands_and_do_not_request_startup_dialogs() {
    let (root, client, spec) = crate::local::omp::tests::fixture();
    std::fs::create_dir_all(root.path().join("agent")).unwrap();
    std::fs::write(root.path().join("agent/startup-dialog"), "true").unwrap();
    std::fs::write(
        root.path().join("agent/plugins.json"),
        serde_json::json!([{"name":"record","source":"extension","description":"Project time tracking"}])
            .to_string(),
    )
    .unwrap();
    let commands = tokio::time::timeout(std::time::Duration::from_secs(5), client.commands(&spec))
        .await
        .unwrap()
        .unwrap();
    assert!(
        commands
            .iter()
            .any(|command| command["name"] == "record" && command["kind"] == "command")
    );
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(!calls.contains("extension_ui_response"));
    assert!(!calls.contains("\"type\": \"prompt\""));
    assert!(!calls.contains("--no-extensions"));
    assert!(calls.contains("--no-ui"));
}

/// Through the real execution service (lanes, RPC decode, wait watch): a native command that
/// holds `get_state` until its dialog is answered must not block the approval response.
/// A concurrent finish wait reports the permission, then reports idle after the answer.
#[tokio::test]
async fn approval_responses_stay_concurrent_with_a_waiting_native_command() {
    use crate::test_support::{Fixture, worker_with_client};
    use model::workspace::registry::{ProjectRegistry, WorkspaceRegistry};
    let (root, client, _spec) = crate::local::omp::tests::fixture();
    std::fs::create_dir_all(root.path().join("agent")).unwrap();
    std::fs::write(
        root.path().join("agent/dialog.json"),
        serde_json::json!({"method":"input","title":"Approval while waiting"}).to_string(),
    )
    .unwrap();
    let fixture = Fixture::new();
    // Route the OMP fixture cwd through a registered workspace like any real creation.
    let projects = persistence::storage::registry::FileBackedProjectRegistry::new(
        fixture.root.path().join("projects.json"),
    );
    projects.initialize().unwrap();
    let workspaces = persistence::storage::registry::FileBackedWorkspaceRegistry::new(
        fixture.root.path().join("workspaces.json"),
    );
    workspaces.initialize().unwrap();
    projects
        .upsert(
            &serde_json::from_value(serde_json::json!({
                "projectId":"project-omp","rootPath":root.path(),"kind":"non_git",
                "displayName":"omp","createdAt":"2026-11-11T00:00:00Z",
                "updatedAt":"2026-11-11T00:00:00Z","archivedAt":null
            }))
            .unwrap(),
        )
        .unwrap();
    workspaces
        .upsert(
            &serde_json::from_value(serde_json::json!({
                "workspaceId":"wks_omp000000000000","projectId":"project-omp","cwd":root.path(),
                "kind":"local_checkout","displayName":"main",
                "createdAt":"2026-11-11T00:00:00Z","updatedAt":"2026-11-11T00:00:00Z",
                "archivedAt":null
            }))
            .unwrap(),
            domain::workspace::registry::WorkspaceMutationContext::default(),
        )
        .unwrap();
    let (execution, _registry) = worker_with_client(
        &fixture,
        model::creation::Creations::default(),
        Box::new(client),
    );
    let created = execution
        .execute(
            "agent.create.request",
            serde_json::json!({"config":{"provider":"omp","cwd":root.path(),"title":"Waiting"}}),
        )
        .await
        .unwrap();
    let id = created["agentId"].as_str().unwrap();
    execution
        .execute(
            "agent.message.send.request",
            serde_json::json!({"agentId":id,"text":"interactive-fixture"}),
        )
        .await
        .unwrap();
    let request = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let snapshot = execution
                .execute("agent.get.request", serde_json::json!({"agentId":id}))
                .await
                .unwrap();
            if let Some(request) = snapshot["agent"]["pendingPermissions"][0].as_object() {
                return Value::Object(request.clone());
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("dialog must publish while the native command waits");
    // Finish waits return the permission state without waiting for native completion.
    // The native prompt itself is still awaiting this answer.
    let waiting = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        execution.execute(
            "agent.finish.wait.request",
            serde_json::json!({"agentId":id,"timeoutMs":10000}),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(waiting["status"], "permission");
    assert_eq!(
        waiting["final"]["pendingPermissions"][0]["id"],
        request["id"]
    );
    // The approval response goes through the session lane while the command still waits.
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        execution.execute(
            "agent.permission.resolve.request",
            serde_json::json!({"agentId":id,"requestId":request["id"],
                "response":{"behavior":"allow","updatedInput":{"answers":{
                    "Approval while waiting":"Approved"}}}}),
        ),
    )
    .await
    .expect("approval response blocked behind the waiting native command")
    .unwrap();
    let result = execution
        .execute(
            "agent.finish.wait.request",
            serde_json::json!({"agentId":id,"timeoutMs":5000}),
        )
        .await
        .unwrap();
    assert_eq!(result["status"], "idle");
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(calls.contains("extension_ui_response"));
    execution.shutdown().await.unwrap();
}

/// A real subprocess that raises a startup question, then rejects initialization.
#[cfg(unix)]
const FAILING_STARTUP_PEER: &str = r"#!/usr/bin/env python3
import json
import os
from pathlib import Path
import sys

root = Path(os.environ['PI_CODING_AGENT_DIR'])
root.mkdir(parents=True, exist_ok=True)
(root / 'pid').write_text(str(os.getpid()))
def emit(value):
    print(json.dumps(value), flush=True)
emit({'type':'ready', 'supportedProtocolVersions':[2]})
emit({'type':'extension_ui_request', 'id':'startup-choice',
      'method':'input', 'title':'Startup input'})
pending = None
for line in sys.stdin:
    command = json.loads(line)
    if command['type'] == 'negotiate_protocol':
        pending = command
    elif command['type'] == 'extension_ui_response':
        emit({'type':'response', 'id':pending['id'], 'command':pending['type'],
              'success':False, 'error':'startupFailed'})
        sys.exit(1)
";

#[cfg(unix)]
fn failing_startup_fixture() -> (tempfile::TempDir, OmpClient, AgentSessionSpec) {
    use std::os::unix::fs::PermissionsExt;
    let (root, mut client, spec) = crate::local::omp::tests::fixture();
    let program = root.path().join("startup.py");
    std::fs::write(&program, FAILING_STARTUP_PEER).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    client.program = program;
    (root, client, spec)
}

#[cfg(unix)]
#[tokio::test]
async fn startup_failure_commits_one_durable_error_and_releases_the_logical_turn() {
    use crate::service::agent_manager::{AgentManager, AgentRegistration};
    use crate::storage::timeline::Timeline;
    use domain::agent_runtime::{AgentRuntimeStatus, registry::AgentRuntimeRegistry};
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

    let (root, client, spec) = failing_startup_fixture();
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    let timeline = Timeline::open(&root.path().join("timeline.sqlite")).unwrap();
    let mut manager = AgentManager::new(Box::new(registry.clone())).with_timeline(timeline.clone());
    manager.register_client(Box::new(client)).unwrap();
    manager
        .create("startup-agent", &spec, AgentRegistration::default())
        .await
        .unwrap();
    manager
        .send("startup-agent", "Never admitted")
        .await
        .unwrap();
    let turn = manager.active_turn("startup-agent").unwrap().to_owned();
    manager
        .permission(
            "startup-agent",
            "startup-choice",
            &serde_json::json!({"behavior":"deny"}),
            PermissionContext::default(),
        )
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while manager.active_turn("startup-agent").is_some() {
            manager.poll().await.unwrap();
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("startup failure must settle through the normal manager drain");
    manager.poll().await.unwrap();
    manager.close_all().await.unwrap();
    drop(manager);
    drop(timeline);

    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    let record = registry.get("startup-agent").unwrap().unwrap();
    assert_eq!(record.last_status, AgentRuntimeStatus::Error);
    assert!(record.last_error.is_some());
    assert!(
        record.persistence.is_none(),
        "a failed draft has no native receipt"
    );
    let timeline = Timeline::open(&root.path().join("timeline.sqlite")).unwrap();
    let (_, entries) = timeline.read("startup-agent").unwrap();
    let failures: Vec<_> = entries
        .iter()
        .filter(|entry| entry.entry.item["type"] == "error")
        .collect();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].entry.turn_id.as_deref(), Some(turn.as_str()));
    assert_eq!(
        failures[0].entry.item["message"].as_str(),
        record.last_error.as_deref()
    );
    assert!(
        !entries
            .iter()
            .any(|entry| entry.entry.item["type"] == "user_message")
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn cancelling_bootstrap_reaps_the_process_closes_pipes_and_is_idempotent() {
    let (root, client, spec) = failing_startup_fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let pid = std::fs::read_to_string(root.path().join("agent/pid")).unwrap();
    let process = std::path::PathBuf::from(format!("/proc/{pid}"));
    let pipes: Vec<_> = (0..3)
        .map(|fd| std::fs::read_link(process.join(format!("fd/{fd}"))).unwrap())
        .collect();
    let turn = session
        .start_turn("Never replay", &spec.config)
        .await
        .unwrap();

    session.cancel_turn(&turn).await.unwrap();
    session.cancel_turn(&turn).await.unwrap();
    session.close().await.unwrap();
    session.service_realtime().await.unwrap();
    let mut events = Vec::new();
    while let Some(event) = session.poll_turn().unwrap() {
        events.push(event);
    }
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, AgentTurnEvent::Cancelled))
            .count(),
        1
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentTurnEvent::Failed | AgentTurnEvent::Completed(_)))
    );
    assert!(session.pending_permissions().is_empty());
    assert!(session.persistence().is_none());
    assert_eq!(
        session.start_turn("Later", &spec.config).await,
        Err(AgentSessionError::Unavailable)
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let open_pipes = std::fs::read_dir("/proc/self/fd")
                .unwrap()
                .filter_map(Result::ok)
                .filter_map(|entry| std::fs::read_link(entry.path()).ok())
                .any(|path| pipes.contains(&path));
            if !process.exists() && !open_pipes {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("cancel must reap the child (not just signal it) and close all pipe owners");
}

#[tokio::test]
async fn close_releases_a_ready_session_that_has_not_been_adopted() {
    let (_root, client, spec) = crate::local::omp::tests::fixture();
    let inner = session::open(&client, &spec, None, None).await.unwrap();
    let (_sender, receiver) = mpsc::channel(1);
    let (finished, ready) = oneshot::channel();
    finished.send(Ok(inner)).unwrap();
    let mut session = Session {
        receiver,
        ready,
        task: None,
        inner: None,
        writer: None,
        dialogs: Dialogs::default(),
        events: VecDeque::new(),
        input: None,
        turns: None,
        closed: false,
        failed: false,
        failure: None,
    };
    let turn = session
        .start_turn("Never replay", &spec.config)
        .await
        .unwrap();
    session.cancel_turn(&turn).await.unwrap();
    assert!(
        matches!(
            session.ready.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        ),
        "cancel must not retain a live native session in the completed startup channel"
    );
    assert!(session.persistence().is_none());
}

#[tokio::test]
async fn cancelled_bootstrap_remains_a_draft_after_reopening_durable_state() {
    use crate::service::agent_manager::{AgentManager, AgentRegistration};
    use crate::storage::timeline::Timeline;
    use domain::agent_runtime::registry::AgentRuntimeRegistry;
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

    let (root, client, spec) = crate::local::omp::tests::fixture();
    std::fs::create_dir_all(root.path().join("agent")).unwrap();
    std::fs::write(root.path().join("agent/startup-dialog"), "true").unwrap();
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    let timeline = Timeline::open(&root.path().join("timeline.sqlite")).unwrap();
    let mut manager = AgentManager::new(Box::new(registry)).with_timeline(timeline.clone());
    manager.register_client(Box::new(client.clone())).unwrap();
    manager
        .create("startup-agent", &spec, AgentRegistration::default())
        .await
        .unwrap();
    manager.send("startup-agent", "Never replay").await.unwrap();
    manager.cancel("startup-agent").await.unwrap();
    manager.cancel("startup-agent").await.unwrap();
    manager.poll().await.unwrap();
    assert!(manager.active_turn("startup-agent").is_none());
    manager.close_all().await.unwrap();
    drop(manager);
    drop(timeline);

    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    let record = registry.get("startup-agent").unwrap().unwrap();
    assert!(record.last_error.is_none());
    assert!(record.persistence.is_none());
    assert!(
        record
            .runtime_info
            .as_ref()
            .is_none_or(|info| info.session_id.is_none()),
        "cancelled initialization must not manufacture a successful native identity"
    );
    let timeline = Timeline::open(&root.path().join("timeline.sqlite")).unwrap();
    assert!(timeline.read("startup-agent").unwrap().1.is_empty());
    assert!(
        client
            .list_sessions(&crate::ports::native_history::ListOptions {
                cwd: Some(spec.cwd.clone()),
                scan_limit: 10,
            })
            .await
            .unwrap()
            .is_empty()
    );
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(!calls.contains("Never replay"));
}
