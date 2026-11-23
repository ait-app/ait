//! Actual WebSocket coverage for concurrent native export cancellation and disconnect.

use std::sync::atomic::{AtomicUsize, Ordering};

use domain::agent_runtime::registry::AgentRuntimeRegistry;
use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentRuntimeInfo};
use model::workspace::registry::{ProjectRegistry, WorkspaceRegistry};
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
use persistence::storage::registry::{FileBackedProjectRegistry, FileBackedWorkspaceRegistry};
use provider::service::agent_execution::{AgentExecution, ExecutionDependencies};
use provider::service::agent_manager::AgentManager;
use provider::service::agent_runtime::AgentRuntimeDirectory;
use provider::service::agents::Agents;
use provider::skills::local::LocalSkills;
use provider::skills::service::Skills;
use provider_core::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionSpec,
};

use super::*;

#[derive(Debug, Default)]
struct State {
    starts: AtomicUsize,
    cancels: AtomicUsize,
}

#[derive(Clone, Debug)]
struct Client(Arc<State>);

#[async_trait::async_trait]
impl AgentClient for Client {
    fn provider(&self) -> &'static str {
        "test-native-export"
    }
    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        Ok(true)
    }
    async fn create_session(
        &self,
        _: &AgentSessionSpec,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        Ok(Box::new(Session(self.0.clone())))
    }
    async fn resume_session(
        &self,
        _: &AgentPersistenceHandle,
        spec: &AgentSessionSpec,
        _: AgentResumePurpose,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        self.create_session(spec).await
    }
}

#[derive(Debug)]
struct Session(Arc<State>);

#[async_trait::async_trait]
impl AgentSession for Session {
    fn provider(&self) -> &'static str {
        "test-native-export"
    }
    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        None
    }
    async fn runtime_info(&mut self) -> Result<StoredAgentRuntimeInfo, AgentSessionError> {
        Ok(StoredAgentRuntimeInfo {
            provider: self.provider().to_owned(),
            session_id: None,
            model: None,
            thinking_option_id: None,
            mode_id: None,
            extra: None,
        })
    }
    fn begin_native_html_export(&mut self) -> Result<(), AgentSessionError> {
        self.0.starts.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn cancel_native_html_export(&mut self) {
        self.0.cancels.fetch_add(1, Ordering::SeqCst);
    }
    async fn close(&mut self) -> Result<(), AgentSessionError> {
        Ok(())
    }
}

#[derive(Debug)]
struct Silent;

#[async_trait::async_trait]
impl model::summary::SummaryGenerator for Silent {
    async fn generate(
        &self,
        _: domain::summary::SummaryRequest,
    ) -> Result<Value, domain::summary::SummaryError> {
        Err(domain::summary::SummaryError::Unavailable)
    }
    fn shutdown(&self) {}
}

fn services(root: &std::path::Path, state: Arc<State>) -> Services {
    let registry = FileBackedAgentRuntimeRegistry::new(root.join("agents.json"));
    let workspaces = FileBackedWorkspaceRegistry::new(root.join("workspaces.json"));
    let projects = FileBackedProjectRegistry::new(root.join("projects.json"));
    registry.initialize().unwrap();
    workspaces.initialize().unwrap();
    projects.initialize().unwrap();
    projects.upsert(&serde_json::from_value(json!({
        "projectId":"project-1", "rootPath":root, "kind":"non_git", "displayName":"project",
        "createdAt":"2026-09-24T00:00:00Z", "updatedAt":"2026-09-24T00:00:00Z", "archivedAt":null
    })).unwrap()).unwrap();
    workspaces
        .upsert(
            &serde_json::from_value(json!({
                "workspaceId":"wks_0123456789abcdef", "projectId":"project-1", "cwd":root,
                "kind":"local_checkout", "displayName":"main", "createdAt":"2026-09-24T00:00:00Z",
                "updatedAt":"2026-09-24T00:00:00Z", "archivedAt":null
            }))
            .unwrap(),
            domain::workspace::registry::WorkspaceMutationContext::default(),
        )
        .unwrap();
    let mut manager = AgentManager::new(Box::new(registry.clone()));
    provider::Providers::empty(root)
        .with(|_| Client(state))
        .register(&mut manager)
        .unwrap();
    let execution = AgentExecution::spawn(ExecutionDependencies {
        manager,
        directory: AgentRuntimeDirectory::new(
            Box::new(registry.clone()),
            Box::new(workspaces.clone()),
            Box::new(projects.clone()),
        ),
        registry: Box::new(registry),
        workspaces: Box::new(workspaces),
        projects: Box::new(projects),
        import_directory: None,
        workspace_automation: None,
        lifetime: Arc::new(()),
    })
    .unwrap();
    let skills = root.join("skills");
    let targets = ["a", "b", "c"].map(|name| skills.join(name));
    Services {
        provider: Some(provider::Service::new(provider::Dependencies {
            agents: Agents::new(Box::new(
                provider::storage::SqliteCatalog::open(root).unwrap(),
            )),
            skills: Skills::new(Box::new(
                LocalSkills::new(&skills.join("source"), &targets, &skills.join("state")).unwrap(),
            )),
            execution,
            summary_generator: Arc::new(Silent),
        })),
        ..Services::default()
    }
}

async fn wait_for(counter: &AtomicUsize, expected: usize) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while counter.load(Ordering::SeqCst) < expected {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("native export state did not advance");
}

async fn reply(socket: &mut Socket, id: &str) -> Value {
    loop {
        let message = receive(socket).await;
        if message["request_id"] == id {
            return message;
        }
    }
}

#[tokio::test]
async fn native_export_allows_same_socket_requests_and_cancels_on_abort_or_disconnect() {
    for single in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = Arc::new(State::default());
        let fixture = Fixture::with_services(services(root.path(), state.clone())).await;
        let mut socket = fixture.socket().await;
        let capabilities = json!([
            "connection.single.v1",
            "agent.create.request",
            "agent.native_html.export.request",
            "agent.native_html.export.cancel.request",
            "agent.permission.resolve.request"
        ]);
        send(&mut socket, json!({"type":"hello", "client_id":"export-test", "protocol":{"major":1,"min_minor":0,"max_minor":4}, "capabilities":capabilities, "required_capabilities":if single {json!(["connection.single.v1"])} else {json!([])}})).await;
        assert_eq!(receive(&mut socket).await["type"], "server_info");
        send(&mut socket, json!({"type":"request","request_id":"create","method":"agent.create.request","params":{"config":{"provider":"test-native-export","cwd":root.path()}}})).await;
        let created = reply(&mut socket, "create").await;
        let agent = created["result"]["agentId"]
            .as_str()
            .unwrap_or_else(|| panic!("created agent: {created}"));
        let operation = uuid::Uuid::new_v4().to_string();
        send(&mut socket, json!({"type":"request","request_id":"export","method":"agent.native_html.export.request","params":{"agentId":agent,"operationId":operation}})).await;
        wait_for(&state.starts, 1).await;
        send(&mut socket, json!({"type":"request","request_id":"permission","method":"agent.permission.resolve.request","params":{"agentId":agent,"requestId":"missing","response":{"behavior":"deny"}}})).await;
        assert_eq!(
            reply(&mut socket, "permission").await["type"],
            "error",
            "same socket must remain responsive"
        );
        send(&mut socket, json!({"type":"request","request_id":"duplicate","method":"agent.native_html.export.request","params":{"agentId":agent,"operationId":operation}})).await;
        assert_eq!(
            reply(&mut socket, "duplicate").await["code"],
            "idempotency_conflict"
        );
        send(&mut socket, json!({"type":"request","request_id":"cancel","method":"agent.native_html.export.cancel.request","params":{"agentId":agent,"operationId":operation}})).await;
        let mut cancelled = false;
        let mut stopped = false;
        while !cancelled || !stopped {
            let response = receive(&mut socket).await;
            if response["request_id"] == "cancel" {
                cancelled = response["result"]["released"] == true;
            }
            if response["request_id"] == "export" {
                stopped = response["type"] == "error";
            }
        }
        wait_for(&state.cancels, 1).await;
        let early = uuid::Uuid::new_v4().to_string();
        send(&mut socket, json!({"type":"request","request_id":"early-cancel","method":"agent.native_html.export.cancel.request","params":{"agentId":agent,"operationId":early}})).await;
        assert_eq!(
            reply(&mut socket, "early-cancel").await["result"]["released"],
            true
        );
        send(&mut socket, json!({"type":"request","request_id":"late-start","method":"agent.native_html.export.request","params":{"agentId":agent,"operationId":early}})).await;
        assert_eq!(reply(&mut socket, "late-start").await["type"], "error");
        assert_eq!(state.starts.load(Ordering::SeqCst), 1);
        let disconnected = uuid::Uuid::new_v4().to_string();
        send(&mut socket, json!({"type":"request","request_id":"disconnect","method":"agent.native_html.export.request","params":{"agentId":agent,"operationId":disconnected}})).await;
        wait_for(&state.starts, 2).await;
        socket.close(None).await.unwrap();
        wait_for(&state.cancels, 2).await;
        fixture.stop().await;
    }
}
