use std::sync::atomic::{AtomicUsize, Ordering};

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use tokio::sync::{Notify, Semaphore};

use super::*;
use crate::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionFuture, AgentSessionSpec,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operation {
    Create,
    Resume,
    History,
    Discovery,
}

#[derive(Debug)]
struct Gate {
    operation: Operation,
    calls: AtomicUsize,
    entered: Notify,
    release: Semaphore,
}

impl Gate {
    fn new(operation: Operation) -> Arc<Self> {
        Arc::new(Self {
            operation,
            calls: AtomicUsize::new(0),
            entered: Notify::new(),
            release: Semaphore::new(0),
        })
    }

    async fn hold(&self, operation: Operation) {
        if self.operation == operation && self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            self.entered.notify_one();
            self.release.acquire().await.unwrap().forget();
        }
    }
}

#[derive(Debug)]
struct GatedClient {
    native: crate::local::codex::CodexClient,
    gate: Arc<Gate>,
}

impl AgentClient for GatedClient {
    fn provider(&self) -> &'static str {
        "codex"
    }
    fn is_available(&self) -> AgentSessionFuture<'_, bool> {
        self.native.is_available()
    }
    fn settings(&self, config: &StoredAgentConfig) -> Value {
        self.native.settings(config)
    }
    fn validate_selection<'a>(&'a self, spec: &'a AgentSessionSpec) -> AgentSessionFuture<'a, ()> {
        self.native.validate_selection(spec)
    }
    fn discover<'a>(
        &'a self,
        cwd: &'a str,
    ) -> AgentSessionFuture<'a, crate::protocol::provider::Details> {
        Box::pin(async move {
            self.gate.hold(Operation::Discovery).await;
            self.native.discover(cwd).await
        })
    }
    fn history<'a>(
        &'a self,
        handle: &'a AgentPersistenceHandle,
        cwd: &'a str,
    ) -> AgentSessionFuture<'a, Vec<crate::protocol::timeline::NativeItem>> {
        Box::pin(async move {
            self.gate.hold(Operation::History).await;
            self.native.history(handle, cwd).await
        })
    }
    fn create_session<'a>(
        &'a self,
        spec: &'a AgentSessionSpec,
    ) -> AgentSessionFuture<'a, Box<dyn AgentSession>> {
        Box::pin(async move {
            self.gate.hold(Operation::Create).await;
            self.native.create_session(spec).await
        })
    }
    fn resume_session<'a>(
        &'a self,
        handle: &'a AgentPersistenceHandle,
        spec: &'a AgentSessionSpec,
        purpose: AgentResumePurpose,
    ) -> AgentSessionFuture<'a, Box<dyn AgentSession>> {
        Box::pin(async move {
            self.gate.hold(Operation::Resume).await;
            self.native.resume_session(handle, spec, purpose).await
        })
    }
}

fn gated(fixture: &Fixture, gate: &Arc<Gate>) -> AgentExecution {
    worker_with_client(
        fixture,
        model::creation::Creations::default(),
        Box::new(GatedClient {
            native: fixture.client(),
            gate: gate.clone(),
        }),
    )
    .0
}

async fn entered(gate: &Gate) {
    tokio::time::timeout(Duration::from_secs(5), gate.entered.notified())
        .await
        .unwrap();
}

#[tokio::test]
async fn slow_create_does_not_block_another_session_read_turn_or_completion() {
    let fixture = Fixture::new();
    let gate = Gate::new(Operation::Create);
    let execution = gated(&fixture, &gate);
    let first = {
        let execution = execution.clone();
        let cwd = fixture.cwd.clone();
        tokio::spawn(async move {
            execution
                .execute(
                    "agent.create.request",
                    json!({"config":{"provider":"codex","cwd":cwd}}),
                )
                .await
        })
    };
    entered(&gate).await;
    let second = tokio::time::timeout(Duration::from_secs(5), create(&execution, &fixture))
        .await
        .unwrap();
    let id = second["agentId"].as_str().unwrap();
    assert!(!first.is_finished());
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"independent"}),
        )
        .await
        .unwrap();
    let finished = tokio::time::timeout(
        Duration::from_secs(5),
        execution.execute("agent.finish.wait.request", json!({"agentId":id})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(finished["lastMessage"], "Echo: independent");
    let read = execution
        .execute("agent.timeline.get.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert!(read["entries"].is_array());
    assert!(!first.is_finished());
    gate.release.add_permits(1);
    assert_ne!(first.await.unwrap().unwrap()["agentId"], second["agentId"]);
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn slow_discovery_does_not_block_directory_reads_or_native_events() {
    let fixture = Fixture::new();
    let gate = Gate::new(Operation::Discovery);
    let execution = gated(&fixture, &gate);
    let agent = create(&execution, &fixture).await;
    let id = agent["agentId"].as_str().unwrap();
    let initial = execution
        .execute("provider.snapshot.get.request", json!({"cwd":fixture.cwd}))
        .await
        .unwrap();
    assert_eq!(initial["entries"][0]["status"], "loading");
    entered(&gate).await;
    let read = tokio::time::timeout(
        Duration::from_secs(5),
        execution.execute("agent.get.request", json!({"agentId":id})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(read["agent"]["id"], id);
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"streaming"}),
        )
        .await
        .unwrap();
    let finished = tokio::time::timeout(
        Duration::from_secs(5),
        execution.execute("agent.finish.wait.request", json!({"agentId":id})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(finished["lastMessage"], "Echo: streaming");
    let repeated = execution
        .execute("provider.snapshot.get.request", json!({"cwd":fixture.cwd}))
        .await
        .unwrap();
    assert_eq!(repeated["entries"][0]["status"], "loading");
    assert_eq!(gate.calls.load(Ordering::SeqCst), 1);
    // Shutdown cancels discovery even though its external gate has never opened.
    tokio::time::timeout(Duration::from_secs(5), execution.shutdown())
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn concurrent_resumes_share_one_writer_and_leave_other_reads_available() {
    let fixture = Fixture::new();
    let (original, _) = worker(&fixture);
    let agent = create(&original, &fixture).await;
    original.shutdown().await.unwrap();
    let gate = Gate::new(Operation::Resume);
    let execution = gated(&fixture, &gate);
    let first = {
        let execution = execution.clone();
        let handle = agent["agent"]["persistence"].clone();
        tokio::spawn(async move {
            execution
                .execute("agent.resume.request", json!({"handle":handle}))
                .await
        })
    };
    entered(&gate).await;
    let second = {
        let execution = execution.clone();
        let handle = agent["agent"]["persistence"].clone();
        tokio::spawn(async move {
            execution
                .execute("agent.resume.request", json!({"handle":handle}))
                .await
        })
    };
    let listed = tokio::time::timeout(
        Duration::from_secs(5),
        execution.execute("agent.list.request", json!({})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(listed["entries"].as_array().unwrap().len(), 1);
    assert!(listed["entries"][0]["agent"]["providerUnavailable"].is_boolean());
    assert!(!first.is_finished());
    let waiting = execution
        .execute(
            "agent.finish.wait.request",
            json!({"agentId":agent["agentId"],"timeoutMs":10}),
        )
        .await
        .unwrap();
    assert_eq!(waiting["status"], "timeout");
    assert_eq!(gate.calls.load(Ordering::SeqCst), 1);
    gate.release.add_permits(1);
    assert_eq!(
        first.await.unwrap().unwrap()["agentId"],
        second.await.unwrap().unwrap()["agentId"]
    );
    assert_eq!(gate.calls.load(Ordering::SeqCst), 1);
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn concurrent_cold_history_reads_share_hydration_without_starting_a_writer() {
    let fixture = Fixture::new();
    let (original, _) = worker(&fixture);
    let agent = create(&original, &fixture).await;
    original.shutdown().await.unwrap();
    let gate = Gate::new(Operation::History);
    let execution = gated(&fixture, &gate);
    let first = {
        let execution = execution.clone();
        let id = agent["agentId"].clone();
        tokio::spawn(async move {
            execution
                .execute("agent.timeline.get.request", json!({"agentId":id}))
                .await
        })
    };
    entered(&gate).await;
    let second = {
        let execution = execution.clone();
        let id = agent["agentId"].clone();
        tokio::spawn(async move {
            execution
                .execute("agent.timeline.get.request", json!({"agentId":id}))
                .await
        })
    };
    assert_eq!(gate.calls.load(Ordering::SeqCst), 1);
    gate.release.add_permits(1);
    first.await.unwrap().unwrap();
    second.await.unwrap().unwrap();
    assert_eq!(gate.calls.load(Ordering::SeqCst), 1);
    let view = execution
        .execute("agent.get.request", json!({"agentId":agent["agentId"]}))
        .await
        .unwrap();
    assert_eq!(view["agent"]["status"], "closed");
    execution.shutdown().await.unwrap();
}

async fn fenced(execution: &AgentExecution, id: &str) {
    let owners = execution.0.template.lock().unwrap().owners.clone();
    let lane = owners.agent(id).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while owners.startup(&lane).unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn archive_and_delete_wait_for_the_related_resume_and_do_not_block_other_sessions() {
    for method in ["agent.archive.request", "agent.delete.request"] {
        let fixture = Fixture::new();
        let (original, _) = worker(&fixture);
        let agent = create(&original, &fixture).await;
        original.shutdown().await.unwrap();
        let gate = Gate::new(Operation::Resume);
        let execution = gated(&fixture, &gate);
        let resume = {
            let execution = execution.clone();
            let handle = agent["agent"]["persistence"].clone();
            tokio::spawn(async move {
                execution
                    .execute("agent.resume.request", json!({"handle":handle}))
                    .await
            })
        };
        entered(&gate).await;
        let mutation = {
            let execution = execution.clone();
            let id = agent["agentId"].clone();
            tokio::spawn(async move { execution.execute(method, json!({"agentId":id})).await })
        };
        fenced(&execution, agent["agentId"].as_str().unwrap()).await;
        let independent =
            tokio::time::timeout(Duration::from_secs(5), create(&execution, &fixture))
                .await
                .unwrap();
        assert!(!mutation.is_finished());
        gate.release.add_permits(1);
        resume.await.unwrap().unwrap();
        mutation.await.unwrap().unwrap();
        let view = execution
            .execute("agent.get.request", json!({"agentId":agent["agentId"]}))
            .await;
        if method == "agent.delete.request" {
            let view = view.unwrap();
            assert!(view["agent"].is_null());
            assert!(view["error"].is_string());
        } else {
            assert!(view.unwrap()["agent"]["archivedAt"].is_string());
        }
        assert_eq!(
            execution
                .execute(
                    "agent.get.request",
                    json!({"agentId":independent["agentId"]})
                )
                .await
                .unwrap()["agent"]["status"],
            "idle"
        );
        execution.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn workspace_retirement_includes_a_creation_whose_native_factory_has_not_completed() {
    let fixture = Fixture::new();
    let gate = Gate::new(Operation::Create);
    let execution = gated(&fixture, &gate);
    let id = uuid::Uuid::new_v4().to_string();
    let creating = {
        let execution = execution.clone();
        let id = id.clone();
        let cwd = fixture.cwd.clone();
        tokio::spawn(async move {
            execution
                .execute(
                    "agent.create.request",
                    json!({"agentId":id,"config":{"provider":"codex","cwd":cwd}}),
                )
                .await
        })
    };
    entered(&gate).await;
    let retiring = {
        let execution = execution.clone();
        tokio::spawn(async move {
            execution
                .execute("internal.workspace.retire", json!(["wks_0123456789abcdef"]))
                .await
        })
    };
    fenced(&execution, &id).await;
    assert!(!retiring.is_finished());
    gate.release.add_permits(1);
    creating.await.unwrap().unwrap();
    let retired = retiring.await.unwrap().unwrap();
    assert_eq!(retired, json!([id]));
    let view = execution
        .execute("agent.get.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert_eq!(view["agent"]["status"], "closed");
    assert!(view["agent"]["archivedAt"].is_string());
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn global_session_budget_counts_an_inflight_factory_and_is_released_by_delete() {
    let fixture = Fixture::new();
    let gate = Gate::new(Operation::Create);
    let execution = gated(&fixture, &gate);
    let pending = {
        let execution = execution.clone();
        let cwd = fixture.cwd.clone();
        tokio::spawn(async move {
            execution
                .execute(
                    "agent.create.request",
                    json!({"config":{"provider":"codex","cwd":cwd}}),
                )
                .await
        })
    };
    entered(&gate).await;
    let mut ids = Vec::new();
    for _ in 0..31 {
        ids.push(create(&execution, &fixture).await["agentId"].clone());
    }
    assert_eq!(
        execution
            .execute(
                "agent.create.request",
                json!({"config":{"provider":"codex","cwd":fixture.cwd}})
            )
            .await,
        Err(ErrorCode::UnsupportedCapability)
    );
    assert_eq!(gate.calls.load(Ordering::SeqCst), 32);
    execution
        .execute("agent.delete.request", json!({"agentId":ids[0]}))
        .await
        .unwrap();
    create(&execution, &fixture).await;
    assert_eq!(gate.calls.load(Ordering::SeqCst), 33);
    gate.release.add_permits(1);
    pending.await.unwrap().unwrap();
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn automatic_archive_fences_a_child_factory_and_waits_for_all_related_writers() {
    let fixture = Fixture::new();
    let gate = Gate::new(Operation::Create);
    gate.calls.store(1, Ordering::SeqCst);
    let execution = gated(&fixture, &gate);
    let parent = execution
        .execute(
            "agent.create.request",
            json!({"config":{"provider":"codex","cwd":fixture.cwd},"autoArchive":true}),
        )
        .await
        .unwrap();
    gate.calls.store(0, Ordering::SeqCst);
    let child = {
        let execution = execution.clone();
        let parent = parent["agentId"].clone();
        let cwd = fixture.cwd.clone();
        tokio::spawn(async move {
            execution
                .execute(
                    "agent.create.request",
                    json!({"callerAgentId":parent,"config":{"provider":"codex","cwd":cwd}}),
                )
                .await
        })
    };
    entered(&gate).await;
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":parent["agentId"],"text":"finish"}),
        )
        .await
        .unwrap();
    fenced(&execution, parent["agentId"].as_str().unwrap()).await;
    let waiting = {
        let execution = execution.clone();
        let id = parent["agentId"].clone();
        tokio::spawn(async move {
            execution
                .execute("agent.finish.wait.request", json!({"agentId":id}))
                .await
        })
    };
    create(&execution, &fixture).await;
    assert!(!waiting.is_finished());
    gate.release.add_permits(1);
    let child = child.await.unwrap().unwrap();
    let finished = tokio::time::timeout(Duration::from_secs(5), waiting)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(finished["final"]["status"], "closed");
    let child = execution
        .execute("agent.get.request", json!({"agentId":child["agentId"]}))
        .await
        .unwrap();
    assert!(child["agent"]["archivedAt"].is_string());
    assert_eq!(child["agent"]["status"], "closed");
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn late_history_cannot_recreate_a_deleted_agent() {
    let fixture = Fixture::new();
    let (original, _) = worker(&fixture);
    let agent = create(&original, &fixture).await;
    original.shutdown().await.unwrap();
    let gate = Gate::new(Operation::History);
    let execution = gated(&fixture, &gate);
    let history = {
        let execution = execution.clone();
        let id = agent["agentId"].clone();
        tokio::spawn(async move {
            execution
                .execute("agent.timeline.get.request", json!({"agentId":id}))
                .await
        })
    };
    entered(&gate).await;
    execution
        .0
        .registry
        .remove(agent["agentId"].as_str().unwrap())
        .unwrap();
    gate.release.add_permits(1);
    assert_eq!(history.await.unwrap(), Err(ErrorCode::AgentNotFound));
    assert!(execution.0.registry.list().unwrap().is_empty());
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn creation_revalidates_workspace_after_a_delayed_native_factory() {
    let fixture = Fixture::new();
    let gate = Gate::new(Operation::Create);
    let execution = gated(&fixture, &gate);
    let creating = {
        let execution = execution.clone();
        let cwd = fixture.cwd.clone();
        tokio::spawn(async move {
            execution
                .execute(
                    "agent.create.request",
                    json!({"config":{"provider":"codex","cwd":cwd}}),
                )
                .await
        })
    };
    entered(&gate).await;
    let workspaces = execution.0.template.lock().unwrap().workspaces.clone();
    let mut workspace = workspaces.get("wks_0123456789abcdef").unwrap().unwrap();
    workspace.archived_at = Some("2026-10-06T00:00:00Z".to_owned());
    workspaces
        .upsert(
            &workspace,
            domain::workspace::registry::WorkspaceMutationContext::default(),
        )
        .unwrap();
    gate.release.add_permits(1);
    assert_eq!(creating.await.unwrap(), Err(ErrorCode::InvalidMessage));
    let records = execution.0.registry.list().unwrap();
    assert_eq!(records.len(), 1);
    assert!(records[0].archived_at.is_some());
    assert_eq!(
        records[0].last_status,
        domain::agent_runtime::AgentRuntimeStatus::Closed
    );
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn committed_reader_does_not_start_native_hydration_after_its_cache_marker_changes() {
    let fixture = Fixture::new();
    let (original, _) = worker(&fixture);
    let agent = create(&original, &fixture).await;
    original
        .execute(
            "agent.message.send.request",
            json!({"agentId":agent["agentId"],"text":"stored"}),
        )
        .await
        .unwrap();
    original
        .execute(
            "agent.finish.wait.request",
            json!({"agentId":agent["agentId"]}),
        )
        .await
        .unwrap();
    original.shutdown().await.unwrap();
    let gate = Gate::new(Operation::History);
    let execution = gated(&fixture, &gate);
    let mut reader = execution.0.template.lock().unwrap().fork(None);
    let read = tokio::time::timeout(
        Duration::from_secs(5),
        reader.execute(
            "agent.timeline.get.request",
            json!({"agentId":agent["agentId"]}),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!read["entries"].as_array().unwrap().is_empty());
    assert_eq!(gate.calls.load(Ordering::SeqCst), 0);
    let cold = {
        let execution = execution.clone();
        let id = agent["agentId"].clone();
        tokio::spawn(async move {
            execution
                .execute("agent.timeline.get.request", json!({"agentId":id}))
                .await
        })
    };
    entered(&gate).await;
    gate.release.add_permits(1);
    cold.await.unwrap().unwrap();
    assert_eq!(gate.calls.load(Ordering::SeqCst), 1);
    execution.shutdown().await.unwrap();
}
