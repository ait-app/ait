//! Same-socket child control and permission delivery against a deterministic adapter.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use domain::agent_runtime::registry::AgentRuntimeRegistry;
use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
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
    AgentTurnEvent,
};

use super::*;

#[derive(Debug, Default)]
struct State {
    controls: AtomicUsize,
    control_admitted: tokio::sync::Notify,
    answers: AtomicUsize,
    pending: AtomicBool,
    events: Mutex<VecDeque<AgentTurnEvent>>,
    child: Mutex<Option<provider_core::ports::controls::NativeSubagent>>,
}

#[derive(Clone, Debug)]
struct Client(Arc<State>);

#[async_trait::async_trait]
impl AgentClient for Client {
    fn provider(&self) -> &'static str {
        "test-child-controls"
    }
    fn validate_config(&self, _: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        Ok(())
    }
    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        Ok(true)
    }
    async fn create_session(
        &self,
        spec: &AgentSessionSpec,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        Ok(Box::new(Session {
            state: self.0.clone(),
            cwd: spec.cwd.clone(),
        }))
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
struct Session {
    state: Arc<State>,
    cwd: String,
}

#[async_trait::async_trait]
impl AgentSession for Session {
    fn provider(&self) -> &'static str {
        "test-child-controls"
    }
    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        Some(AgentPersistenceHandle {
            provider: self.provider().to_owned(),
            session_id: "native-root".to_owned(),
            native_handle: None,
            metadata: None,
        })
    }
    async fn runtime_info(&mut self) -> Result<StoredAgentRuntimeInfo, AgentSessionError> {
        Ok(StoredAgentRuntimeInfo {
            provider: self.provider().to_owned(),
            session_id: Some("native-root".to_owned()),
            model: None,
            thinking_option_id: None,
            mode_id: None,
            extra: None,
        })
    }
    async fn start_turn(
        &mut self,
        _: &str,
        _: &StoredAgentConfig,
    ) -> Result<String, AgentSessionError> {
        let child = provider_core::ports::controls::NativeSubagent {
            id: "native-child".to_owned(),
            parent_id: "native-root".to_owned(),
            cwd: self.cwd.clone(),
            persistence: None,
            descriptor: json!({"id":"native-child", "status":"running", "updatedAt":"2026-11-23T00:00:00Z"}),
        };
        *self.state.child.lock().unwrap() = Some(child.clone());
        self.state.pending.store(true, Ordering::SeqCst);
        let mut events = self.state.events.lock().unwrap();
        events.push_back(AgentTurnEvent::Subagent(
            provider_core::ports::controls::SubagentEvent::Upsert(child),
        ));
        events.push_back(AgentTurnEvent::PermissionRequested(
            json!({"id":"permission-1", "toolName":"fixture"}),
        ));
        Ok("turn-1".to_owned())
    }
    fn subagents(&self) -> Vec<provider_core::ports::controls::NativeSubagent> {
        self.state.child.lock().unwrap().iter().cloned().collect()
    }
    fn pending_permissions(&self) -> Vec<Value> {
        if self.state.pending.load(Ordering::SeqCst) {
            vec![json!({"id":"permission-1", "toolName":"fixture"})]
        } else {
            Vec::new()
        }
    }
    fn begin_child_guide(
        &mut self,
        _: &str,
        _: &str,
    ) -> Result<provider_core::ports::controls::ChildControlReceipt, AgentSessionError> {
        self.state.controls.fetch_add(1, Ordering::SeqCst);
        self.state.control_admitted.notify_one();
        Ok(provider_core::ports::controls::ChildControlReceipt {
            token: "receipt".to_owned(),
        })
    }
    async fn respond_permission(&mut self, id: &str, _: &Value) -> Result<(), AgentSessionError> {
        if id != "permission-1" || !self.state.pending.swap(false, Ordering::SeqCst) {
            return Err(AgentSessionError::Rejected);
        }
        self.state.answers.fetch_add(1, Ordering::SeqCst);
        self.state
            .events
            .lock()
            .unwrap()
            .push_back(AgentTurnEvent::ChildControl {
                token: "receipt".to_owned(),
                outcome: provider_core::ports::controls::ChildControlOutcome::Guide(Ok(
                    provider_core::ports::controls::ChildGuideOutcome::Accepted,
                )),
            });
        Ok(())
    }
    fn poll_turn(&mut self) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        Ok(self.state.events.lock().unwrap().pop_front())
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

async fn reply(socket: &mut Socket, id: &str) -> Value {
    loop {
        let message = receive(socket).await;
        if message["request_id"] == id {
            return message;
        }
    }
}

#[tokio::test]
async fn child_controls_do_not_block_permission_on_the_same_socket() {
    for single in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = Arc::new(State::default());
        let fixture = Fixture::with_services(services(root.path(), state.clone())).await;
        let mut socket = fixture.socket().await;
        let capabilities = json!([
            "connection.single.v1",
            "agent.create.request",
            "agent.message.send.request",
            "agent.subagents.list.request",
            "agent.subagents.guide.request",
            "agent.subagents.stop.request",
            "agent.permission.resolve.request"
        ]);
        send(&mut socket, json!({"type":"hello", "client_id":"child-test", "protocol":{"major":1,"min_minor":0,"max_minor":4}, "capabilities":capabilities, "required_capabilities":if single {json!(["connection.single.v1"])} else {json!([])}})).await;
        assert_eq!(receive(&mut socket).await["type"], "server_info");
        send(&mut socket, json!({"type":"request","request_id":"create","method":"agent.create.request","params":{"config":{"provider":"test-child-controls","cwd":root.path()}}})).await;
        let created = reply(&mut socket, "create").await;
        let parent = created["result"]["agentId"]
            .as_str()
            .unwrap_or_else(|| panic!("create: {created}"));
        send(&mut socket, json!({"type":"request","request_id":"input","method":"agent.message.send.request","params":{"agentId":parent,"text":"start"}})).await;
        assert_eq!(
            reply(&mut socket, "input").await["result"]["accepted"],
            true
        );
        send(&mut socket, json!({"type":"request","request_id":"list","method":"agent.subagents.list.request","params":{"parentAgentId":parent}})).await;
        let listed = reply(&mut socket, "list").await;
        let child = listed["result"]["agents"][0]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("child: {listed}"));
        let revision = listed["result"]["agents"][0]["observation"]["revision"]
            .as_u64()
            .unwrap();
        send(&mut socket, json!({"type":"request","request_id":"guide","method":"agent.subagents.guide.request","params":{"parentAgentId":parent,"childAgentId":child,"message":"help","expectedRevision":revision}})).await;
        tokio::time::timeout(Duration::from_secs(5), state.control_admitted.notified())
            .await
            .expect("child control was not admitted");
        send(&mut socket, json!({"type":"request","request_id":"permission","method":"agent.permission.resolve.request","params":{"agentId":parent,"requestId":"permission-1","response":{"behavior":"allow"}}})).await;
        let mut permission = false;
        let mut guide = false;
        while !permission || !guide {
            let result = receive(&mut socket).await;
            if result["request_id"] == "permission" {
                assert_eq!(result["type"], "response", "{result}");
                permission = true;
            }
            if result["request_id"] == "guide" {
                assert_eq!(result["result"]["disposition"], "accepted", "{result}");
                guide = true;
            }
        }
        assert_eq!(state.controls.load(Ordering::SeqCst), 1);
        assert_eq!(state.answers.load(Ordering::SeqCst), 1);
        socket.close(None).await.unwrap();
        fixture.stop().await;
    }
}
