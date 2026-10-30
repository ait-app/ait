//! Shared fixtures for tests that need real workspaces and share settings.

use std::net::SocketAddr;
use std::sync::Arc;

use domain::workspace::lifecycle::WorkspaceCreation;
use filesystem::git::local::provisioning::LocalDirectorySource;
use metadata::directory::service::directory::{Directory, DirectoryDependencies};
use model::share::{Accepted, Outcome, ResourceKind, Visitor};
use persistence::storage::project_config::LocalProjectConfigStore;
use persistence::storage::project_icon::LocalProjectIconStore;
use persistence::storage::registry::{FileBackedProjectRegistry, FileBackedWorkspaceRegistry};
use serde_json::json;
use uuid::Uuid;

use crate::{Api, Services};

pub(crate) const TOKEN: &str = "in-process-test-token-at-least-32-characters";

/// An API whose directory holds two projects in separate directories. The shared project
/// owns the `shared` workspace at its root and a `sibling` workspace outside its root, the
/// way managed worktrees live; the other project owns the `other` workspace.
pub(crate) struct Workspaces {
    pub(crate) root: tempfile::TempDir,
    pub(crate) api: Api,
    pub(crate) shared: String,
    pub(crate) sibling: String,
    pub(crate) other: String,
    pub(crate) project: String,
    pub(crate) other_project: String,
    pub(crate) group: Uuid,
}

impl Workspaces {
    /// Build the API for `address` with a shared project and an unrelated project.
    pub(crate) fn new(address: SocketAddr) -> Self {
        let root = tempfile::tempdir().unwrap();
        for name in ["shared", "sibling", "other", "registry"] {
            std::fs::create_dir(root.path().join(name)).unwrap();
        }
        let registry = root.path().join("registry");
        let directory = Directory::new(DirectoryDependencies {
            projects: Box::new(FileBackedProjectRegistry::new(
                registry.join("projects.json"),
            )),
            workspaces: Box::new(FileBackedWorkspaceRegistry::new(
                registry.join("workspaces.json"),
            )),
            source: Box::new(LocalDirectorySource),
            config_store: Box::new(LocalProjectConfigStore),
            icon_store: Box::new(LocalProjectIconStore::new(registry.join("icons"))),
            server_id: "test-server".into(),
        });
        let path = |name: &str| root.path().join(name).to_str().unwrap().to_owned();
        let open = |name: &str| {
            directory
                .open_workspace(&path(name), "2026-10-30T00:00:00Z")
                .unwrap()
        };
        let (shared, other) = (open("shared"), open("other"));
        let sibling = directory
            .create_workspace(WorkspaceCreation {
                path: &path("sibling"),
                title: None,
                project_id: Some(&shared.project_id),
                workspace_id: None,
                expects_initial_agent: false,
                timestamp: "2026-10-30T00:00:00Z",
            })
            .unwrap();
        let mut api = Api::new(
            address,
            "test-server".into(),
            "instance".into(),
            TOKEN.into(),
            Services::default(),
        )
        .unwrap();
        let state = Arc::get_mut(&mut api.shared).unwrap();
        Arc::get_mut(&mut state.metadata).unwrap().directory =
            Some(crate::shared_service(directory));
        Self {
            root,
            api,
            shared: shared.workspace_id,
            sibling: sibling.workspace_id,
            other: other.workspace_id,
            project: shared.project_id,
            other_project: other.project_id,
            group: Uuid::new_v4(),
        }
    }

    /// Canonical directory of `name` below the fixture root.
    pub(crate) fn path(&self, name: &str) -> std::path::PathBuf {
        self.root.path().join(name).canonicalize().unwrap()
    }

    /// Canonical directory of the shared workspace and root of the shared project.
    pub(crate) fn shared_path(&self) -> std::path::PathBuf {
        self.path("shared")
    }

    /// Archive workspace `id` directly in the directory.
    pub(crate) fn archive(&self, id: &str) {
        self.api
            .shared
            .metadata
            .directory
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .archive_workspace(id, "2026-10-30T01:00:00Z")
            .unwrap();
    }

    /// Configure a group-wide share of the shared workspace and accept it.
    pub(crate) async fn accept_share(&self) -> Visitor {
        self.accept(ResourceKind::Workspace, &self.shared).await
    }

    /// Configure a group-wide share of `kind` and `key` and accept it.
    pub(crate) async fn accept(&self, kind: ResourceKind, key: &str) -> Visitor {
        let request = serde_json::from_value(json!({
            "resourceKind": kind,
            "resourceKey": key,
            "groupId": self.group,
            "permission": "operate",
            "audience": "group",
        }))
        .unwrap();
        self.api.shared.shares.set(request).await.unwrap();
        let share_id = Uuid::new_v4();
        self.api.share_book().apply(Outcome {
            revision: None,
            accepted: vec![Accepted {
                share_id,
                resource_kind: kind,
                resource_key: key.to_owned(),
                group_id: self.group,
                generation: 1,
                missing_recipients: Vec::new(),
            }],
            rejected: Vec::new(),
            groups: Vec::new(),
        });
        Visitor {
            user_id: Uuid::new_v4(),
            display_name: Some("Guest".into()),
            group_id: self.group,
            share_id,
            resource_kind: kind,
            resource_key: key.to_owned(),
            permission: "operate".into(),
        }
    }
}

/// An unassigned-port-free address for in-process APIs that never listen.
pub(crate) fn address() -> SocketAddr {
    "127.0.0.1:7316".parse().unwrap()
}
