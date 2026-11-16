use super::*;
use domain::agent_runtime::registry::AgentRuntimeRegistry;
use model::workspace::registry::{ProjectRegistry, WorkspaceRegistry};
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
use persistence::storage::registry::{FileBackedProjectRegistry, FileBackedWorkspaceRegistry};
use provider::service::agent_execution::{AgentExecution, ExecutionDependencies};
use provider::service::agent_manager::AgentManager;
use provider::service::agent_runtime::AgentRuntimeDirectory;
use provider::storage::side::SideStore;

#[tokio::test]
async fn installed_btw_methods_reach_execution_through_the_complete_request_chain() {
    let (api, execution, _root) = installed_execution();
    for (method, params, expected) in [
        (
            "agent.side_question.history.request",
            json!({"agentId":"stored-agent"}),
            json!({"records":[],"omitted":0}),
        ),
        (
            "agent.side_question.ask.request",
            json!({"agentId":"stored-agent","requestId":"empty-question","question":""}),
            json!({"receipt":"rejected","reason":"invalid_question"}),
        ),
        (
            "agent.side_question.cancel.request",
            json!({"agentId":"stored-agent"}),
            json!({"cancelled":false}),
        ),
    ] {
        let response = dispatch_rpc(&api, method, params).await;
        assert_eq!(response["type"], "response", "{method}: {response}");
        for (field, value) in expected.as_object().unwrap() {
            assert_eq!(&response["result"][field], value, "{method}: {response}");
        }
    }
    let response = dispatch_rpc(
        &api,
        "agent.side_question.transfer.request",
        json!({"agentId":"stored-agent","recordId":"missing-topic"}),
    )
    .await;
    assert_eq!(response["type"], "error");
    assert_eq!(response["code"], "invalid_message");
    execution.shutdown().await.unwrap();
}

async fn dispatch_rpc(api: &Api, method: &str, params: Value) -> Value {
    let (outbound, mut receiver) = Outbound::new();
    let mut subscriptions = ConnectionSubscriptions::default();
    let mut context = context(method, &api.shared, &outbound);
    context.request.params = params;
    request(context, &api.shared, &mut subscriptions)
        .await
        .unwrap();
    let response = decode(receiver.try_recv().unwrap().message);
    assert_eq!(response["request_id"], "request-1", "{method}: {response}");
    assert!(receiver.try_recv().is_err(), "{method} replied twice");
    assert_eq!(subscriptions.len(), 0);
    response
}

fn installed_execution() -> (Api, AgentExecution, tempfile::TempDir) {
    let root = tempfile::tempdir().unwrap();
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    registry.initialize().unwrap();
    registry
        .upsert(
            &serde_json::from_value(json!({
                "id":"stored-agent", "provider":"offline-test", "cwd":root.path(),
                "createdAt":"2026-11-13T00:00:00Z", "updatedAt":"2026-11-13T00:00:00Z",
                "labels":{}, "lastStatus":"closed"
            }))
            .unwrap(),
        )
        .unwrap();
    let workspaces = FileBackedWorkspaceRegistry::new(root.path().join("workspaces.json"));
    let projects = FileBackedProjectRegistry::new(root.path().join("projects.json"));
    workspaces.initialize().unwrap();
    projects.initialize().unwrap();
    let store = SideStore::open(&root.path().join("side-questions.sqlite3")).unwrap();
    let execution = AgentExecution::spawn(ExecutionDependencies {
        manager: AgentManager::new(Box::new(registry.clone())).with_side_store(store),
        directory: AgentRuntimeDirectory::new(
            Box::new(registry.clone()),
            Box::new(workspaces.clone()),
            Box::new(projects.clone()),
        ),
        registry: Box::new(registry),
        workspaces: Box::new(workspaces),
        projects: Box::new(projects),
        lifetime: Arc::new(()),
        import_directory: None,
        workspace_automation: None,
    })
    .unwrap();
    let mut api = api();
    let shared = Arc::get_mut(&mut api.shared).unwrap();
    let previous = &shared.provider;
    shared.provider = Arc::new(provider::dispatch::State {
        runtime: previous.runtime.clone(),
        agents: previous.agents.clone(),
        skills: previous.skills.clone(),
        agent_runtime: previous.agent_runtime.clone(),
        agent_execution: Some(execution.clone()),
        summary_source: previous.summary_source.clone(),
        directory_changes: previous.directory_changes.clone(),
        has_terminals: previous.has_terminals,
    });
    (api, execution, root)
}
