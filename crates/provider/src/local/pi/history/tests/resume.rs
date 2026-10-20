use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::sync::Arc;

use domain::agent_runtime::registry::AgentRuntimeRegistry;
use model::workspace::registry::{ProjectRegistry, WorkspaceRegistry};
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
use persistence::storage::registry::{FileBackedProjectRegistry, FileBackedWorkspaceRegistry};

use crate::service::agent_execution::{AgentExecution, ExecutionDependencies};
use crate::service::agent_manager::AgentManager;
use crate::service::agent_runtime::AgentRuntimeDirectory;

fn worker(
    root: &Path,
    cwd: &str,
    client: PiClient,
) -> (AgentExecution, FileBackedAgentRuntimeRegistry) {
    let registry = FileBackedAgentRuntimeRegistry::new(root.join("agents.json"));
    registry.initialize().unwrap();
    let projects = FileBackedProjectRegistry::new(root.join("projects.json"));
    projects.initialize().unwrap();
    let workspaces = FileBackedWorkspaceRegistry::new(root.join("workspaces.json"));
    workspaces.initialize().unwrap();
    projects.upsert(&serde_json::from_value(json!({
        "projectId":"project-1","rootPath":cwd,"kind":"non_git","displayName":"project",
        "createdAt":"2026-09-24T00:00:00Z","updatedAt":"2026-09-24T00:00:00Z","archivedAt":null
    })).unwrap()).unwrap();
    workspaces
        .upsert(
            &serde_json::from_value(json!({
                "workspaceId":"wks_0123456789abcdef","projectId":"project-1","cwd":cwd,
                "kind":"local_checkout","displayName":"main","createdAt":"2026-09-24T00:00:00Z",
                "updatedAt":"2026-09-24T00:00:00Z","archivedAt":null
            }))
            .unwrap(),
            domain::workspace::registry::WorkspaceMutationContext::default(),
        )
        .unwrap();
    let mut manager = AgentManager::new(Box::new(registry.clone()));
    manager.register_client(Box::new(client)).unwrap();
    let execution = AgentExecution::spawn(ExecutionDependencies {
        manager,
        directory: AgentRuntimeDirectory::new(
            Box::new(registry.clone()),
            Box::new(workspaces.clone()),
            Box::new(projects.clone()),
        ),
        registry: Box::new(registry.clone()),
        workspaces: Box::new(workspaces),
        projects: Box::new(projects),
        import_directory: None,
        workspace_automation: None,
        lifetime: Arc::new(()),
    })
    .unwrap();
    (execution, registry)
}

#[tokio::test]
async fn unknown_resume_preserves_symlink_spelling_from_metadata_and_overrides() {
    for override_cwd in [false, true] {
        let (directory, mut client, _file, _cwd) = fixture();
        let project = directory.path().join("project");
        let linked = directory.path().join("linked-project");
        std::fs::create_dir(&project).unwrap();
        symlink(&project, &linked).unwrap();
        let cwd = project.canonicalize().unwrap().to_str().unwrap().to_owned();
        let folder = project_directory(&directory.path().join("sessions"), &linked).unwrap();
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("session.jsonl");
        write(
            &file,
            &cwd,
            vec![
                user("u", Value::Null, "linked"),
                assistant("a", "u", json!([{"type":"text","text":"done"}])),
            ],
        );
        let program = directory.path().join("pi");
        std::fs::write(&program, include_str!("../../tests/fixtures/pi.py")).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        client.program = program;
        let (execution, registry) = worker(directory.path(), &cwd, client.clone());
        let params = if override_cwd {
            json!({"handle":{"provider":"pi","sessionId":SESSION,"metadata":{"cwd":cwd}},"overrides":{"cwd":linked}})
        } else {
            json!({"handle":{"provider":"pi","sessionId":SESSION,"metadata":{"cwd":linked}}})
        };
        let response = execution
            .execute("agent.resume.request", params)
            .await
            .unwrap();
        let id = response["agentId"].as_str().unwrap();
        let record = registry.get(id).unwrap().unwrap();
        assert_eq!(record.cwd, cwd);
        assert_eq!(
            record
                .persistence
                .as_ref()
                .unwrap()
                .metadata
                .as_ref()
                .unwrap()["nativeCwd"],
            json!(linked)
        );
        execution.shutdown().await.unwrap();
        let renamed = folder.join("renamed.jsonl");
        std::fs::rename(file, &renamed).unwrap();
        let (restarted, _) = worker(directory.path(), &cwd, client);
        restarted
            .execute("agent.resume.request", json!({"handle":record.persistence}))
            .await
            .unwrap();
        restarted
            .execute("agent.refresh.request", json!({"agentId":id}))
            .await
            .unwrap();
        restarted.shutdown().await.unwrap();
    }
}
