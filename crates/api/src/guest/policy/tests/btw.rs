use super::*;
use domain::agent_runtime::registry::AgentRuntimeRegistry;
use model::workspace::registry::{ProjectRegistry, WorkspaceRegistry};
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
use persistence::storage::registry::{FileBackedProjectRegistry, FileBackedWorkspaceRegistry};
use provider::service::agent_execution::{AgentExecution, ExecutionDependencies};
use provider::service::agent_manager::AgentManager;
use provider::service::agent_runtime::AgentRuntimeDirectory;

async fn guest_with_agent_directory() -> (Guest, AgentExecution) {
    let mut fixture = Workspaces::new(address());
    let root = fixture.root.path().join("registry");
    let registry = FileBackedAgentRuntimeRegistry::new(root.join("agents.json"));
    registry.initialize().expect("agent registry");
    let projects = FileBackedProjectRegistry::new(root.join("projects.json"));
    let workspaces = FileBackedWorkspaceRegistry::new(root.join("workspaces.json"));
    projects.initialize().expect("existing project registry");
    workspaces
        .initialize()
        .expect("existing workspace registry");
    let execution = AgentExecution::spawn(ExecutionDependencies {
        manager: AgentManager::new(Box::new(registry.clone())),
        directory: AgentRuntimeDirectory::new(
            Box::new(registry.clone()),
            Box::new(workspaces.clone()),
            Box::new(projects.clone()),
        ),
        registry: Box::new(registry.clone()),
        workspaces: Box::new(workspaces),
        projects: Box::new(projects),
        lifetime: Arc::new(()),
        import_directory: None,
        workspace_automation: None,
    })
    .expect("read-only agent worker");
    let state = Arc::get_mut(&mut fixture.api.shared).expect("unshared API fixture");
    let previous = &state.provider;
    state.provider = Arc::new(provider::dispatch::State {
        runtime: previous.runtime.clone(),
        agents: previous.agents.clone(),
        skills: previous.skills.clone(),
        agent_runtime: previous.agent_runtime.clone(),
        agent_execution: Some(execution.clone()),
        summary_source: previous.summary_source.clone(),
        directory_changes: previous.directory_changes.clone(),
        has_terminals: previous.has_terminals,
    });
    let visitor = fixture
        .accept(ResourceKind::Workspace, &fixture.shared)
        .await;
    let session = admit(&fixture.api.shared, &visitor.encode())
        .await
        .expect("guest admission");
    for (id, workspace, guest_created) in [
        ("owner-agent", &fixture.shared, false),
        ("guest-agent", &fixture.shared, true),
        ("foreign-agent", &fixture.other, true),
    ] {
        let labels = if guest_created {
            json!({GUEST_SHARE: visitor.share_id.to_string(), GUEST_USER: visitor.user_id.to_string()})
        } else {
            json!({})
        };
        registry.upsert(&serde_json::from_value(json!({
            "id":id, "provider":"codex", "cwd":fixture.shared_path(), "workspaceId":workspace,
            "createdAt":"2026-11-13T00:00:00Z", "updatedAt":"2026-11-13T00:00:00Z",
            "labels":labels, "lastStatus":"closed"
        })).expect("stored agent record")).expect("register scoped agent");
    }
    (
        Guest {
            fixture,
            session,
            visitor,
        },
        execution,
    )
}

#[tokio::test]
async fn statistics_and_btw_reads_are_shared_but_side_question_writes_require_ownership() {
    let (guest, execution) = guest_with_agent_directory().await;
    for method in [
        "agent.statistics.get.request",
        "agent.side_question.history.request",
        "agent.side_question.transfer.request",
    ] {
        let forwarded = guest
            .forwarded(method, json!({"agentId":"owner-agent","recordId":"topic"}))
            .await;
        assert_eq!(forwarded["agentId"], "owner-agent");
    }
    for method in [
        "agent.side_question.ask.request",
        "agent.side_question.cancel.request",
    ] {
        assert_eq!(
            guest
                .refused(method, json!({"agentId":"owner-agent","question":"why?"}))
                .await,
            ErrorCode::Forbidden
        );
        let forwarded = guest
            .forwarded(method, json!({"agentId":"guest-agent","question":"why?"}))
            .await;
        assert_eq!(forwarded["agentId"], "guest-agent");
    }
    execution
        .shutdown()
        .await
        .expect("directory worker shutdown");
}

#[tokio::test]
async fn statistics_api_dispatch_reads_saved_facts_without_a_native_provider() {
    let (guest, execution) = guest_with_agent_directory().await;
    let method = "agent.statistics.get.request";
    let params = guest
        .forwarded(method, json!({"agentId":"owner-agent"}))
        .await;

    let statistics = crate::connection::rpc::execute(&guest.fixture.api.shared, method, params)
        .await
        .expect("statistics must traverse the API dispatch chain");

    assert_eq!(statistics["agentId"], "owner-agent");
    assert_eq!(statistics["provider"], "codex");
    assert_eq!(statistics["status"], "closed");
    assert_eq!(statistics["usageScope"], "provider_snapshot");
    assert!(statistics["configuredModel"].is_null());
    assert!(statistics["effectiveModel"].is_null());
    assert!(statistics["usage"]["inputTokens"].is_null());
    assert_eq!(
        crate::connection::rpc::execute(
            &guest.fixture.api.shared,
            method,
            json!({"agentId":"missing-agent"}),
        )
        .await,
        Err(ErrorCode::AgentNotFound)
    );
    execution
        .shutdown()
        .await
        .expect("directory worker shutdown");
}

#[tokio::test]
async fn statistics_and_btw_stay_inside_the_workspace_even_for_guest_created_agents() {
    let (guest, execution) = guest_with_agent_directory().await;
    for method in [
        "agent.statistics.get.request",
        "agent.side_question.ask.request",
        "agent.side_question.cancel.request",
        "agent.side_question.history.request",
        "agent.side_question.transfer.request",
    ] {
        for id in ["foreign-agent", "missing-agent"] {
            assert_eq!(
                guest
                    .refused(method, json!({"agentId":id,"recordId":"topic"}))
                    .await,
                ErrorCode::Forbidden
            );
        }
    }
    execution
        .shutdown()
        .await
        .expect("directory worker shutdown");
}
