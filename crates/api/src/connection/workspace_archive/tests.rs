use std::sync::Arc;

use super::*;
use crate::{Api, Services};
use filesystem::local::provisioning::LocalDirectorySource;
use metadata::service::directory::{Directory, DirectoryDependencies};
use metadata::storage::{
    project_config::LocalProjectConfigStore,
    project_icon::LocalProjectIconStore,
    registry::{FileBackedProjectRegistry, FileBackedWorkspaceRegistry},
};
use model::{Request, outbound::Outbound};

struct Fixture {
    root: std::path::PathBuf,
    api: Api,
    workspace: String,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("ait-archive-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let directory = Directory::new(DirectoryDependencies {
            projects: Box::new(FileBackedProjectRegistry::new(root.join("projects.json"))),
            workspaces: Box::new(FileBackedWorkspaceRegistry::new(
                root.join("workspaces.json"),
            )),
            source: Box::new(LocalDirectorySource),
            config_store: Box::new(LocalProjectConfigStore),
            icon_store: Box::new(LocalProjectIconStore::new(root.join("icons"))),
            server_id: "test-server".into(),
        });
        let workspace = directory
            .open_workspace(root.to_str().unwrap(), "2026-10-07T00:00:00Z")
            .unwrap()
            .workspace_id;
        let mut api = Api::new(
            "127.0.0.1:7316".parse().unwrap(),
            "test-server".into(),
            "instance".into(),
            "in-process-test-token-at-least-32-characters".into(),
            Services::default(),
        )
        .unwrap();
        let shared = Arc::get_mut(&mut api.shared).unwrap();
        Arc::get_mut(&mut shared.metadata).unwrap().directory =
            Some(crate::shared_service(directory));
        Self {
            root,
            api,
            workspace,
        }
    }

    fn context<'a>(&'a self, outbound: &'a Outbound) -> Context<'a> {
        Context {
            request: Request {
                id: "archive".into(),
                method: "workspace.archive.request".into(),
                params: json!({"workspaceId":self.workspace}),
            },
            runtime: &self.api.shared,
            outbound,
            available_subscriptions: 16,
        }
    }

    fn archived(&self) -> bool {
        self.api
            .shared
            .metadata
            .directory
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .list_workspaces()
            .unwrap()
            .iter()
            .find(|workspace| workspace.workspace_id == self.workspace)
            .unwrap()
            .archived_at
            .is_some()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[tokio::test]
async fn archive_waits_for_shared_capacity_before_mutating_metadata() {
    let fixture = Fixture::new();
    let (outbound, _receiver) = Outbound::new();
    let mut context = fixture.context(&outbound);
    let permit = fixture
        .api
        .shared
        .runtime
        .jobs
        .clone()
        .acquire_owned()
        .await
        .unwrap();
    let archive = metadata(&fixture.api.shared, &mut context);
    tokio::pin!(archive);
    assert!(futures_util::poll!(&mut archive).is_pending());
    assert!(!fixture.archived());
    drop(permit);
    let value = archive.await.unwrap();
    assert_eq!(value["workspaceId"], fixture.workspace);
    assert!(value["archivedAt"].is_string());
    assert!(fixture.archived());
}

#[tokio::test]
async fn shutdown_cancels_archive_wait_without_mutating_metadata() {
    let fixture = Fixture::new();
    let (outbound, _receiver) = Outbound::new();
    let mut context = fixture.context(&outbound);
    let _permit = fixture
        .api
        .shared
        .runtime
        .jobs
        .clone()
        .acquire_owned()
        .await
        .unwrap();
    let archive = metadata(&fixture.api.shared, &mut context);
    tokio::pin!(archive);
    assert!(futures_util::poll!(&mut archive).is_pending());
    fixture.api.shared.runtime.cancellation.cancel();
    assert_eq!(archive.await, Err(ErrorCode::ServerDraining));
    assert!(!fixture.archived());
}
